package accounts

import (
	"context"
	"errors"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/stelofinance/stelofinance/database/gensql"
)

type TransactionCode int32

const (
	// Transfers from system accounts to user accounts
	TransactionCodeSysToUser TransactionCode = 0
)

type TransactionInput struct {
	DebitWalletId  int64
	CreditWalletId int64
	Code           TransactionCode
	Memo           *string
	Assets         []TransactionAssets
}

type TransactionAssets struct {
	LedgerId int64
	Amount   int64
}

var ErrInvalidBalance = errors.New("transaction: invalid balance")

// TODO: Need to implement pending TXs
func createTransaction(ctx context.Context, q *gensql.Queries, input TransactionInput) (int64, error) {
	// Create transaction record
	txId, err := q.InsertTransaction(ctx, gensql.InsertTransactionParams{
		DebitWalletID:  input.DebitWalletId,
		CreditWalletID: input.CreditWalletId,
		Code:           int32(input.Code),
		Memo:           input.Memo,
		CreatedAt:      time.Now(),
	})
	if err != nil {
		return 0, err
	}

	// Query both wallets for types
	debitWallet, err := q.GetWallet(ctx, input.DebitWalletId)
	if err != nil {
		return 0, err
	}
	creditWallet, err := q.GetWallet(ctx, input.CreditWalletId)
	if err != nil {
		return 0, err
	}

	for _, a := range input.Assets {
		var debitAccId int64
		var creditAccId int64

		// Debit the debit wallet's accounts. If an account doesn't exist and the debit wallet is
		// a credit type account, return balance error.
		if AccountCode(debitWallet.Code).IsCredit() {
			id, err := q.UpdateCreditAccountDebitsPosted(ctx, gensql.UpdateCreditAccountDebitsPostedParams{
				DebitsPosted: a.Amount,
				WalletID:     input.DebitWalletId,
				Code:         debitWallet.Code,
				LedgerID:     a.LedgerId,
			})
			if errors.Is(err, pgx.ErrNoRows) {
				return 0, ErrInvalidBalance
			}
			debitAccId = id
		} else {
			id, err := q.UpdateDebitAccountDebitsPosted(ctx, gensql.UpdateDebitAccountDebitsPostedParams{
				DebitsPosted: a.Amount,
				WalletID:     input.DebitWalletId,
				Code:         debitWallet.Code,
				LedgerID:     a.LedgerId,
			})
			if errors.Is(err, pgx.ErrNoRows) {
				id, err := q.InsertAccount(ctx, gensql.InsertAccountParams{
					WalletID:       input.DebitWalletId,
					DebitsPending:  0,
					DebitsPosted:   a.Amount,
					CreditsPending: 0,
					CreditsPosted:  0,
					LedgerID:       a.LedgerId,
					Code:           debitWallet.Code,
					Flags:          0,
					CreatedAt:      time.Now(),
				})
				if err != nil {
					return 0, err
				}
				debitAccId = id
			} else {
				debitAccId = id
			}
		}
		// Credit the credit wallet's accounts. If an account doesn't exist and the credit wallet is
		// a debit type account, return balance error.
		if AccountCode(creditWallet.Code).IsCredit() {
			id, err := q.UpdateCreditAccountCreditsPosted(ctx, gensql.UpdateCreditAccountCreditsPostedParams{
				CreditsPosted: a.Amount,
				WalletID:      input.CreditWalletId,
				Code:          creditWallet.Code,
				LedgerID:      a.LedgerId,
			})
			if errors.Is(err, pgx.ErrNoRows) {
				id, err := q.InsertAccount(ctx, gensql.InsertAccountParams{
					WalletID:       input.CreditWalletId,
					DebitsPending:  0,
					DebitsPosted:   0,
					CreditsPending: 0,
					CreditsPosted:  a.Amount,
					LedgerID:       a.LedgerId,
					Code:           creditWallet.Code,
					Flags:          0,
					CreatedAt:      time.Now(),
				})
				if err != nil {
					return 0, err
				}
				creditAccId = id
			} else {
				creditAccId = id
			}
		} else {
			id, err := q.UpdateDebitAccountCreditsPosted(ctx, gensql.UpdateDebitAccountCreditsPostedParams{
				CreditsPosted: a.Amount,
				WalletID:      input.CreditWalletId,
				Code:          creditWallet.Code,
				LedgerID:      a.LedgerId,
			})
			if errors.Is(err, pgx.ErrNoRows) {
				return 0, ErrInvalidBalance
			}
			creditAccId = id
		}

		// Create transfer record
		_, err := q.InsertTransfer(ctx, gensql.InsertTransferParams{
			TransactionID:   txId,
			DebitAccountID:  debitAccId,
			CreditAccountID: creditAccId,
			Amount:          a.Amount,
			PendingID:       nil,
			LedgerID:        a.LedgerId,
			Code:            int32(input.Code),
			Flags:           0,
			CreatedAt:       time.Now(),
		})
		if err != nil {
			return 0, err
		}
	}

	return txId, nil
}
