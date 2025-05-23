package middlewares

import (
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"net/http"
	"strings"

	"github.com/go-chi/chi/v5"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/stelofinance/stelofinance/internal/sessions"
)

func GothicChiAdapter(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		provider := chi.URLParam(r, "provider")
		r = r.WithContext(context.WithValue(r.Context(), "provider", provider))

		next.ServeHTTP(w, r)
	})
}

func AuthSession(logger *slog.Logger, sessionsKV jetstream.KeyValue, authRequired bool) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			cookie, err := r.Cookie("sid")
			if err != nil {
				if !authRequired {
					next.ServeHTTP(w, r)
					return
				}
				w.WriteHeader(http.StatusUnauthorized)
				return
			}

			sid, found := strings.CutPrefix(cookie.Value, "stl_")
			if !found {
				w.WriteHeader(http.StatusBadRequest)
				return
			}

			// Retrieve session data
			sVal, err := getKeyValueWithPattern(r.Context(), sessionsKV, "users.*.sessions."+sid)
			if err != nil {
				if errors.Is(err, ErrKeyNotFound) {
					w.WriteHeader(http.StatusBadRequest)
					return
				}
				w.WriteHeader(http.StatusInternalServerError)
				return
			}

			// Unmarshal data
			var sData sessions.Data
			if err := json.Unmarshal(sVal, &sData); err != nil {
				w.WriteHeader(http.StatusBadRequest)
				return
			}

			// Add session data to request
			r = r.WithContext(sessions.WithSession(r.Context(), &sData))
			next.ServeHTTP(w, r)
		})
	}
}

var ErrKeyNotFound = errors.New("middlewares: key not found")

// TODO: replace with ListKeysFiltered
// getKeyValueWithPattern will search a jetstream kv bucket for the first key that matches a pattern
func getKeyValueWithPattern(ctx context.Context, sessionsKV jetstream.KeyValue, pattern string) ([]byte, error) {
	watcher, err := sessionsKV.Watch(ctx, pattern, jetstream.IgnoreDeletes())
	if err != nil {
		return nil, err
	}
	defer watcher.Stop()

	for {
		select {
		case entry := <-watcher.Updates():
			if entry == nil {
				return nil, ErrKeyNotFound
			}
			return entry.Value(), nil
		case <-ctx.Done():
			return nil, ctx.Err()
		}
	}

}
