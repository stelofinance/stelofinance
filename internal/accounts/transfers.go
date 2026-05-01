package accounts

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
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
var ErrIncompatibleLedgers = errors.New("transaction: incompatible account ledgers")
var ErrMatchingSenderReceiver = errors.New("transaction: sender is receiver")
var ErrMemoExceedsLimit = errors.New("transaction: memo exceeds length limit")

type CreateTransferInput struct {
	SendingId   int64
	ReceivingId int64

	Memo     *string
	LedgerId int64
	Amount   int64
}

func CreateTransfer(ctx context.Context, q *gensql.Queries, nc *nats.Conn, input CreateTransferInput) (int64, EventPublisher, error) {
	publisher := func() error { return nil }

	// Validate asset is >= 1 qty
	if input.Amount < 1 {
		return 0, publisher, ErrInvalidQuantity
	}

	if input.SendingId == input.ReceivingId {
		return 0, publisher, ErrMatchingSenderReceiver
	}

	if input.Memo != nil && len(*input.Memo) > 50 {
		return 0, publisher, ErrMemoExceedsLimit
	}

	// Query both wallets for types
	sendingAcc, err := q.GetAccountById(ctx, input.SendingId)
	if err != nil {
		return 0, publisher, err
	}
	receivingAcc, err := q.GetAccountById(ctx, input.ReceivingId)
	if err != nil {
		return 0, publisher, err
	}

	// Ensure both accounts are for same ledger
	if sendingAcc.LedgerID != receivingAcc.LedgerID {
		return 0, publisher, ErrIncompatibleLedgers
	}

	// Determine TxCode
	trC := AccountCode(sendingAcc.Code).IdentifyTrCode(AccountCode(receivingAcc.Code))
	if trC == -1 {
		return 0, publisher, ErrIncompatibleAccCodes
	}

	// Determine who's creditor/debitor
	creditId, debitId := determineCreditorDebitor(trC, input.SendingId, receivingAcc.ID)
	now := time.Now()

	// Update account balances
	// TODO: implement pending if needed

	// Debit the debit account
	rows, err := q.UpdateDebitsPosted(ctx, gensql.UpdateDebitsPostedParams{
		Quantity: input.Amount,
		ID:       debitId,
	})
	if rows == 0 {
		return 0, publisher, ErrInvalidBalance
	}
	if err != nil {
		return 0, publisher, err
	}
	// Credit the credit account
	rows, err = q.UpdateCreditsPosted(ctx, gensql.UpdateCreditsPostedParams{
		Quantity: input.Amount,
		ID:       creditId,
	})
	if rows == 0 {
		return 0, publisher, ErrInvalidBalance
	}
	if err != nil {
		return 0, publisher, err
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
		return 0, publisher, err
	}

	trEvnt := EventTransfer{
		ID:          creditId,
		DebitAccId:  debitId,
		CreditAccId: creditId,
		Amount:      input.Amount,
		LedgerID:    input.LedgerId,
		Code:        trC,
		Memo:        input.Memo,
		CreatedAt:   now,
	}

	// Create json bytes of tx
	evntBytes, err := json.Marshal(trEvnt)
	if err != nil {
		return 0, publisher, err
	}

	publisher = func() error {
		errGrp := errors.Join(nil)
		errors.Join(errGrp, PublishEvent(nc, trEvnt))
		if err != nil {
			return err
		}

		if sendingAcc.Webhook != nil {
			resp, err := http.Post(*sendingAcc.Webhook, "application/json", bytes.NewBuffer(evntBytes))
			resp.Body.Close()
			errors.Join(errGrp, err)

		}
		if receivingAcc.Webhook != nil {
			resp, _ := http.Post(*receivingAcc.Webhook, "application/json", bytes.NewBuffer(evntBytes))
			resp.Body.Close()
			errors.Join(errGrp, err)
		}

		return errGrp
	}

	return trId, publisher, nil
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

func DetermineSenderReceiver(trC TrCode, creditorId, debitorId int64) (senderId, receiverId int64) {
	switch trC {
	case TrLiability:
		return debitorId, creditorId
	case TrAsset, TrIssue, TrRedeem:
		return creditorId, debitorId
	default:
		// TODO: Should this be handled?
		return creditorId, debitorId
	}
}
