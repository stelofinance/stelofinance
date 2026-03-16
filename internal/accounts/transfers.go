package accounts

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"net/http"
	"strconv"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/stelofinance/stelofinance/database/gensql"
)

type TrCode int32 // Transfer Code

const (
	// Transfers between liability (credit) accounts
	// Credit <-> Credit
	TrLiability TrCode = iota

	// Transfers between asset (debit) accounts
	// Debit <-> Debit
	TrAsset

	// Creation of an asset onto the platform
	// Credit -> Debit
	TrIssue

	// Deletion of an asset from the platform
	// Debit -> Credit
	TrRedeem
)

var ErrTrCodeInvalid = errors.New("Invalid TrCode value")

func (t *TrCode) UnmarshalJSON(data []byte) error {
	var num int32
	if err := json.Unmarshal(data, &num); err != nil {
		return err
	}

	switch TrCode(num) {
	case
		TrLiability,
		TrAsset,
		TrIssue,
		TrRedeem:
		*t = TrCode(num)
	default:
		return ErrTrCodeInvalid
	}

	return nil
}

type TrFlag uint8

const TrFlagNone TrFlag = 0

const (
	TrFlagPending TrFlag = 1 << iota
	TrFlagPostPending
	TrFlagVoidPending
	TrFlagRESERVED4
	TrFlagRESERVED5
	// ...
)

var ErrInvalidQuantity = errors.New("transfer: invalid quantity")
var ErrInvalidBalance = errors.New("transfer: invalid balance")
var ErrIncompatibleAccCodes = errors.New("transaction: incompatible account codes")

type CreateTransferInput struct {
	SendingId   int64
	ReceivingId int64

	Memo     *string
	LedgerId int64
	Amount   int64
}

func CreateTransfer(ctx context.Context, q *gensql.Queries, nc *nats.Conn, input CreateTransferInput) (int64, error) {
	// Validate asset is >= 1 qty
	if input.Amount < 1 {
		return 0, ErrInvalidQuantity
	}

	// Query both wallets for types
	sendingAcc, err := q.GetAccountById(ctx, input.SendingId)
	if err != nil {
		return 0, err
	}
	receivingAcc, err := q.GetAccountById(ctx, input.ReceivingId)
	if err != nil {
		return 0, err
	}

	// Determine TxCode
	trC := AccountCode(sendingAcc.Code).IdentifyTrCode(AccountCode(receivingAcc.Code))
	if trC == -1 {
		return 0, ErrIncompatibleAccCodes
	}

	// Determine who's creditor/debitor
	creditId, debitId := determineCreditorDebitor(trC, input.SendingId, receivingAcc.ID)
	now := time.Now()

	// Update account balances
	suffix := "_posted"
	// if TrFlagPending&input.Flags == TrFlagPending {
	// 	suffix = "_pending"
	// }

	// Debit the debit account
	err = q.UpdateAccountBalance(ctx, gensql.UpdateAccountBalanceParams{
		Field:    "debits" + suffix,
		Quantity: input.Amount,
		ID:       debitId,
	})
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return 0, ErrInvalidBalance
		} else {
			return 0, err
		}
	}
	// Credit the credit account
	err = q.UpdateAccountBalance(ctx, gensql.UpdateAccountBalanceParams{
		Field:    "credits" + suffix,
		Quantity: input.Amount,
		ID:       creditId,
	})
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return 0, ErrInvalidBalance
		} else {
			return 0, err
		}
	}

	// Create transfer record
	trId, err := q.InsertTransfer(ctx, gensql.InsertTransferParams{
		DebitAccountID:  debitId,
		CreditAccountID: creditId,
		Amount:          input.Amount,
		PendingID:       nil,
		LedgerID:        input.LedgerId,
		Code:            int64(trC),
		Flags:           int64(TrFlagNone),
		Memo:            input.Memo,
		CreatedAt:       now,
	})
	if err != nil {
		return 0, err
	}

	// Make types for sending to webhook
	type Transfer struct {
		ID int64 `json:"id"`

		DebitAccId  int64  `json:"debitAccId"`
		CreditAccId int64  `json:"creditAccId"`
		DebitAddr   string `json:"debitAddr"`
		CreditAddr  string `json:"creditAddr"`
		Amount      int64  `json:"amount"`
		LedgerID    int64  `json:"ledgerId"`

		// Flags int64 `json:"flags"`
		Code      int32     `json:"code"`
		Memo      *string   `json:"memo,omitempty"`
		CreatedAt time.Time `json:"createdAt"`
	}

	creditAddr, debitAddr := sendingAcc.Address, receivingAcc.Address
	if sendingAcc.ID != creditId {
		creditAddr, debitAddr = receivingAcc.Address, sendingAcc.Address
	}
	tx := struct {
		ID          int64  `json:"id"`
		DebitAccId  int64  `json:"debitAccId"`
		CreditAccId int64  `json:"creditAccId"`
		DebitAddr   string `json:"debitAddr"`
		CreditAddr  string `json:"creditAddr"`
		Amount      int64  `json:"amount"`
		LedgerID    int64  `json:"ledgerId"`

		// Flags int64 `json:"flags"`
		Code      int32     `json:"code"`
		Memo      *string   `json:"memo,omitempty"`
		CreatedAt time.Time `json:"createdAt"`
	}{
		ID:          trId,
		DebitAccId:  debitId,
		CreditAccId: creditId,
		DebitAddr:   debitAddr,
		CreditAddr:  creditAddr,
		Amount:      input.Amount,
		LedgerID:    input.LedgerId,

		Code:      int32(trC),
		Memo:      input.Memo,
		CreatedAt: now,
	}

	// Create json of tx
	data, err := json.Marshal(tx)
	if err != nil {
		return 0, err
	}

	// Send to NATS and webhook
	go func() {
		nc.Publish("accounts."+strconv.Itoa(int(sendingAcc.ID))+".transactions", data)
		nc.Publish("accounts."+strconv.Itoa(int(receivingAcc.ID))+".transactions", data)

		if sendingAcc.Webhook != nil {
			resp, _ := http.Post(*sendingAcc.Webhook, "application/json", bytes.NewBuffer(data))
			resp.Body.Close()
		}
		if receivingAcc.Webhook != nil {
			resp, _ := http.Post(*receivingAcc.Webhook, "application/json", bytes.NewBuffer(data))
			resp.Body.Close()
		}
	}()

	return trId, nil
}

func determineCreditorDebitor(trC TrCode, sendingId, receivingId int64) (creditId, debitId int64) {
	switch trC {
	case TrLiability:
		return receivingId, sendingId
	case TrAsset, TrIssue, TrRedeem:
		return sendingId, receivingId
	default:
		// TODO: Should this be handled?
		return sendingId, receivingId
	}
}
