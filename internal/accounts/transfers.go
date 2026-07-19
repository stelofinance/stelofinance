package accounts

import (
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/stelofinance/stelofinance/database/gensql"
	"modernc.org/sqlite"
	sqlite3 "modernc.org/sqlite/lib"
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

const MaxIdempotencyKeyLen = 64

var ErrInvalidQuantity = errors.New("transfer: invalid quantity")
var ErrInvalidBalance = errors.New("transfer: invalid balance")
var ErrIncompatibleAccCodes = errors.New("transaction: incompatible account codes")
var ErrIncompatibleLedgers = errors.New("transaction: incompatible account ledgers")
var ErrMatchingSenderReceiver = errors.New("transaction: sender is receiver")
var ErrMemoExceedsLimit = errors.New("transaction: memo exceeds length limit")
var ErrIdempotencyKeyRequired = errors.New("transfer: idempotency key required")
var ErrIdempotencyKeyInvalid = errors.New("transfer: idempotency key invalid")
var ErrIdempotencyConflict = errors.New("transfer: idempotency key conflict")
var ErrIdempotencyRace = errors.New("transfer: idempotency key race")

type CreateTransferInput struct {
	SendingId      int64
	ReceivingId    int64
	Memo           *string
	LedgerId       int64
	Amount         int64
	IdempotencyKey string
}

type CreateTransferResult struct {
	TransferID int64
	Created    bool
	Publish    EventPublisher
}

func CreateTransfer(ctx context.Context, q *gensql.Queries, nc *nats.Conn, input CreateTransferInput) (CreateTransferResult, error) {
	noop := func() error { return nil }
	result := CreateTransferResult{Publish: noop}

	key := strings.TrimSpace(input.IdempotencyKey)
	if key == "" {
		return result, ErrIdempotencyKeyRequired
	}
	if len(key) > MaxIdempotencyKeyLen {
		return result, ErrIdempotencyKeyInvalid
	}

	// Validate asset is >= 1 qty
	if input.Amount < 1 {
		return result, ErrInvalidQuantity
	}

	if input.SendingId == input.ReceivingId {
		return result, ErrMatchingSenderReceiver
	}

	if input.Memo != nil && len(*input.Memo) > 50 {
		return result, ErrMemoExceedsLimit
	}

	reqHash := TransferRequestHash(input.ReceivingId, input.Amount, input.LedgerId, input.Memo)

	// Idempotent replay / conflict check
	existing, err := q.GetTransferIdempotency(ctx, gensql.GetTransferIdempotencyParams{
		AccountID: input.SendingId,
		Key:       key,
	})
	if err == nil {
		if existing.RequestHash != reqHash {
			return result, ErrIdempotencyConflict
		}
		result.TransferID = existing.TransferID
		result.Created = false
		return result, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return result, err
	}

	// Query both wallets for types
	sendingAcc, err := q.GetAccountById(ctx, input.SendingId)
	if err != nil {
		return result, err
	}
	receivingAcc, err := q.GetAccountById(ctx, input.ReceivingId)
	if err != nil {
		return result, err
	}

	// Ensure both accounts are for same ledger
	if sendingAcc.LedgerID != receivingAcc.LedgerID {
		return result, ErrIncompatibleLedgers
	}

	// Determine TxCode
	trC := AccountCode(sendingAcc.Code).IdentifyTrCode(AccountCode(receivingAcc.Code))
	if trC == -1 {
		return result, ErrIncompatibleAccCodes
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
		return result, ErrInvalidBalance
	}
	if err != nil {
		return result, err
	}
	// Credit the credit account
	rows, err = q.UpdateCreditsPosted(ctx, gensql.UpdateCreditsPostedParams{
		Quantity: input.Amount,
		ID:       creditId,
	})
	if rows == 0 {
		return result, ErrInvalidBalance
	}
	if err != nil {
		return result, err
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
		return result, err
	}

	err = q.InsertTransferIdempotency(ctx, gensql.InsertTransferIdempotencyParams{
		AccountID:   input.SendingId,
		Key:         key,
		TransferID:  trId,
		RequestHash: reqHash,
		CreatedAt:   now,
	})
	if err != nil {
		if isUniqueConstraintError(err) {
			// Concurrent request claimed this key first; caller must roll back this txn
			// and re-read the winning transfer.
			return result, ErrIdempotencyRace
		}
		return result, err
	}

	trEvnt := EventTransfer{
		ID:          trId,
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
		return result, err
	}

	publisher := func() error {
		var errGrp error
		errGrp = errors.Join(errGrp, PublishEvent(nc, trEvnt))

		if sendingAcc.Webhook != nil {
			resp, err := http.Post(*sendingAcc.Webhook, "application/json", bytes.NewBuffer(evntBytes))
			if err == nil {
				resp.Body.Close()
			}
			errGrp = errors.Join(errGrp, err)
		}
		if receivingAcc.Webhook != nil {
			resp, err := http.Post(*receivingAcc.Webhook, "application/json", bytes.NewBuffer(evntBytes))
			if err == nil {
				resp.Body.Close()
			}
			errGrp = errors.Join(errGrp, err)
		}

		return errGrp
	}

	result.TransferID = trId
	result.Created = true
	result.Publish = publisher
	return result, nil
}

// TransferRequestHash is the stable fingerprint of a create-transfer intent.
// Used for idempotency conflict detection (same key, different payload → 409).
func TransferRequestHash(receivingId, amount, ledgerId int64, memo *string) string {
	m := ""
	if memo != nil {
		m = *memo
	}
	sum := sha256.Sum256(fmt.Appendf(nil, "%d|%d|%d|%s", receivingId, amount, ledgerId, m))
	return hex.EncodeToString(sum[:])
}

func isUniqueConstraintError(err error) bool {
	var sqliteErr *sqlite.Error
	if !errors.As(err, &sqliteErr) {
		return false
	}
	// Primary result code is SQLITE_CONSTRAINT for unique/PK violations
	// (extended codes still have low byte SQLITE_CONSTRAINT).
	return sqliteErr.Code()&0xff == sqlite3.SQLITE_CONSTRAINT
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
