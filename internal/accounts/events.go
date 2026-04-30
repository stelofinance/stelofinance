package accounts

import (
	"encoding/json"
	"fmt"
	"time"
)

type EventPublisher func() error

type Event interface {
	Subject() string
}

type Publisher interface {
	Publish(subject string, data []byte) error
}

func PublishEvent(p Publisher, e Event) error {
	data, err := json.Marshal(e)
	if err != nil {
		return err
	}

	if err := p.Publish(e.Subject(), data); err != nil {
		return fmt.Errorf("events: publish")
	}

	return nil
}

type EventTransfer struct {
	ID          int64 `json:"id"`
	DebitAccId  int64 `json:"debitAccId"`
	CreditAccId int64 `json:"creditAccId"`

	// DebitAddr   string `json:"debitAddr"`
	// CreditAddr  string `json:"creditAddr"`

	Amount   int64 `json:"amount"`
	LedgerID int64 `json:"ledgerId"`

	// Flags int64 `json:"flags"`
	Code      TrCode    `json:"code"`
	Memo      *string   `json:"memo,omitempty"`
	CreatedAt time.Time `json:"createdAt"`
}

func (e EventTransfer) Subject() string {
	sender, receiver := DetermineSenderReceiver(e.Code, e.CreditAccId, e.DebitAccId)
	// accounts.permissions.{sender_id}.{receiver_id}
	return fmt.Sprintf("accounts.transfers.%v.%v", sender, receiver)
}

type EventAccountPermissionCreated struct {
	PermissionID int64     `json:"id"`
	AccountId    int64     `json:"accId"`
	UserId       int64     `json:"userId"`
	Permissions  int64     `json:"permissions"`
	CreatedAt    time.Time `json:"createdAt"`
}

func (e EventAccountPermissionCreated) Subject() string {
	// accounts.permissions.{account_id}
	return fmt.Sprintf("accounts.permissions.%v", e.AccountId)
}
