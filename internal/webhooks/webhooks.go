package webhooks

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"

	"github.com/nats-io/nats.go/jetstream"
	"github.com/stelofinance/stelofinance/internal/accounts"
	"github.com/stelofinance/stelofinance/internal/logger"
)

const (
	StreamName   = "WEBHOOKS"
	Subject      = "webhooks.deliver"
	ConsumerName = "webhook-deliverer"

	httpTimeout = 10 * time.Second
	ackWait     = 45 * time.Second
	maxDeliver  = 10
	maxAge      = 7 * 24 * time.Hour
	maxInFlight = 8
	userAgent   = "Stelo-Webhooks/1.0"
)

// DeliveryJob is a durable webhook delivery task stored in JetStream.
type DeliveryJob struct {
	AccountID int64                  `json:"accountId"`
	URL       string                 `json:"url"`
	Event     accounts.EventTransfer `json:"event"`
}

// Service enqueues transfer webhooks and runs the delivery worker.
type Service struct {
	js     jetstream.JetStream
	lgr    *logger.Logger
	client *http.Client
}

// New creates a webhook service. Call Ensure before Enqueue or RunWorker.
func New(js jetstream.JetStream, lgr *logger.Logger) *Service {
	return &Service{
		js:  js,
		lgr: lgr,
		client: &http.Client{
			Timeout: httpTimeout,
			CheckRedirect: func(req *http.Request, via []*http.Request) error {
				return http.ErrUseLastResponse
			},
		},
	}
}

// Ensure creates or updates the WEBHOOKS stream and durable pull consumer.
func (s *Service) Ensure(ctx context.Context) error {
	_, err := s.js.CreateOrUpdateStream(ctx, jetstream.StreamConfig{
		Name:      StreamName,
		Subjects:  []string{Subject},
		Retention: jetstream.WorkQueuePolicy,
		Storage:   jetstream.FileStorage,
		MaxAge:    maxAge,
		Replicas:  1,
	})
	if err != nil {
		return fmt.Errorf("webhooks: create stream: %w", err)
	}

	_, err = s.js.CreateOrUpdateConsumer(ctx, StreamName, jetstream.ConsumerConfig{
		Durable:       ConsumerName,
		FilterSubject: Subject,
		AckPolicy:     jetstream.AckExplicitPolicy,
		AckWait:       ackWait,
		MaxDeliver:    maxDeliver,
		// Progressive redelivery delays after AckWait / explicit Nak.
		// MaxDeliver is 10; last interval is reused for remaining attempts.
		BackOff: []time.Duration{
			time.Second,
			5 * time.Second,
			30 * time.Second,
			2 * time.Minute,
			5 * time.Minute,
			15 * time.Minute,
			30 * time.Minute,
			time.Hour,
			2 * time.Hour,
		},
	})
	if err != nil {
		return fmt.Errorf("webhooks: create consumer: %w", err)
	}
	return nil
}

// EnqueueTransferWebhook publishes a durable delivery job for one account webhook.
// Implements accounts.WebhookEnqueuer.
func (s *Service) EnqueueTransferWebhook(ctx context.Context, accountID int64, url string, event accounts.EventTransfer) error {
	job := DeliveryJob{
		AccountID: accountID,
		URL:       url,
		Event:     event,
	}
	data, err := json.Marshal(job)
	if err != nil {
		return fmt.Errorf("webhooks: marshal job: %w", err)
	}

	var pubErr error
	for attempt := range 3 {
		_, pubErr = s.js.Publish(ctx, Subject, data)
		if pubErr == nil {
			return nil
		}
		select {
		case <-ctx.Done():
			return fmt.Errorf("webhooks: publish: %w", ctx.Err())
		case <-time.After(time.Duration(attempt+1) * 50 * time.Millisecond):
		}
	}

	s.log(logger.ErrorLevel, "webhooks: failed to enqueue delivery", map[string]any{
		"error":      pubErr.Error(),
		"transferId": event.ID,
		"accountId":  accountID,
		"url":        url,
	})
	return fmt.Errorf("webhooks: publish: %w", pubErr)
}

// RunWorker consumes webhook jobs and POSTs them until ctx is cancelled.
func (s *Service) RunWorker(ctx context.Context) {
	cons, err := s.js.Consumer(ctx, StreamName, ConsumerName)
	if err != nil {
		s.log(logger.ErrorLevel, "webhooks worker: get consumer failed", map[string]any{
			"error": err.Error(),
		})
		return
	}

	sem := make(chan struct{}, maxInFlight)

	for {
		if ctx.Err() != nil {
			return
		}

		// Fetch with a short wait so we re-check ctx regularly.
		msgs, err := cons.Fetch(maxInFlight, jetstream.FetchMaxWait(2*time.Second))
		if err != nil {
			// Timeout with no messages is normal.
			if ctx.Err() != nil {
				return
			}
			continue
		}

		for msg := range msgs.Messages() {
			if ctx.Err() != nil {
				// Leave message unacked for redelivery after restart.
				return
			}
			sem <- struct{}{}
			go func(m jetstream.Msg) {
				defer func() { <-sem }()
				s.handleMsg(m)
			}(msg)
		}
		if err := msgs.Error(); err != nil && ctx.Err() == nil {
			s.log(logger.WarnLevel, "webhooks worker: fetch batch error", map[string]any{
				"error": err.Error(),
			})
		}
	}
}

func (s *Service) handleMsg(msg jetstream.Msg) {
	var job DeliveryJob
	if err := json.Unmarshal(msg.Data(), &job); err != nil {
		s.log(logger.ErrorLevel, "webhooks: invalid job payload, terminating", map[string]any{
			"error": err.Error(),
		})
		// Poison message: do not retry forever.
		_ = msg.Term()
		return
	}

	meta, _ := msg.Metadata()
	delivered := uint64(0)
	if meta != nil {
		delivered = meta.NumDelivered
	}

	status, err := s.deliver(job)
	if err == nil && status >= 200 && status < 300 {
		if ackErr := msg.Ack(); ackErr != nil {
			s.log(logger.WarnLevel, "webhooks: ack failed", map[string]any{
				"error":      ackErr.Error(),
				"transferId": job.Event.ID,
				"accountId":  job.AccountID,
			})
		}
		return
	}

	data := map[string]any{
		"transferId": job.Event.ID,
		"accountId":  job.AccountID,
		"url":        job.URL,
		"attempt":    delivered,
	}
	if err != nil {
		data["error"] = err.Error()
	} else {
		data["status"] = status
	}

	if delivered >= uint64(maxDeliver) {
		s.log(logger.ErrorLevel, "webhooks: max deliveries exhausted", data)
	} else {
		s.log(logger.WarnLevel, "webhooks: delivery failed, will retry", data)
	}

	// Nak with no delay; consumer BackOff applies on AckWait expiry for unacked
	// messages, but explicit Nak redelivers promptly. Use progressive delay from
	// delivery count when available.
	delay := backoffForAttempt(int(delivered))
	if nakErr := msg.NakWithDelay(delay); nakErr != nil {
		_ = msg.Nak()
	}
}

func (s *Service) deliver(job DeliveryJob) (status int, err error) {
	body, err := json.Marshal(job.Event)
	if err != nil {
		return 0, err
	}

	req, err := http.NewRequest(http.MethodPost, job.URL, bytes.NewReader(body))
	if err != nil {
		return 0, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("User-Agent", userAgent)

	resp, err := s.client.Do(req)
	if err != nil {
		return 0, err
	}
	defer resp.Body.Close()
	_, _ = io.Copy(io.Discard, resp.Body)

	return resp.StatusCode, nil
}

func backoffForAttempt(attempt int) time.Duration {
	// attempt is NumDelivered (1-based on first failure path after first try).
	delays := []time.Duration{
		time.Second,
		5 * time.Second,
		30 * time.Second,
		2 * time.Minute,
		5 * time.Minute,
		15 * time.Minute,
		30 * time.Minute,
		time.Hour,
		2 * time.Hour,
	}
	idx := max(attempt-1, 0)
	if idx >= len(delays) {
		return delays[len(delays)-1]
	}
	return delays[idx]
}

func (s *Service) log(level logger.Level, msg string, data map[string]any) {
	if s.lgr == nil {
		return
	}
	_ = s.lgr.Log(logger.Log{
		Message: msg,
		Data:    data,
		Level:   level,
	})
}
