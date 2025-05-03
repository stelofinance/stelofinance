package accounts

import (
	"context"
	"errors"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/stelofinance/stelofinance/database/gensql"
)

type TxCode int32

const (
	// Transfers from system accounts to user accounts
	TxSysUser TxCode = 0

	// Transaction to/from warehouse collateral
	TxCollateral = 1
)

type TxInput struct {
	DebitWalletId  int64
	CreditWalletId int64
	Code           TxCode
	Memo           *string
	Assets         []TxAssets
}

type TxAssets struct {
	LedgerId int64
	Amount   int64
}

var ErrInvalidBalance = errors.New("transaction: invalid balance")
var ErrInvalidCollatTx = errors.New("transaction: invalid collateral transaction")

// TODO: Need to implement pending TXs
func CreateTransaction(ctx context.Context, q *gensql.Queries, input TxInput) (int64, error) {
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

	// Handle a collateral deposit/withdraw here.
	// This is because the wallets code and the account code don't match.
	if input.Code == TxCollateral {
		// Ensure only Stelo
		if len(input.Assets) != 1 {
			return 0, ErrInvalidCollatTx
		}
		asset := input.Assets[0]
		if asset.LedgerId != 1 { // TODO: Dynamically match this to Stelo ledger id
			return 0, ErrInvalidCollatTx
		}

		// Ensure debit or credit wallet is actually a warehouse.
		if debitWallet.Code != int32(WarehouseAcc) && creditWallet.Code != int32(WarehouseAcc) {
			return 0, ErrInvalidCollatTx
		}

		debitAccCode := AccountCode(debitWallet.Code)
		creditAccCode := AccountCode(creditWallet.Code)

		// Set the correct account codes
		// (warehouse collateral acc doesn't match it's wallet)
		//
		// Also ensure debit or credit wallet is actually a warehouse.
		if debitWallet.Code == int32(WarehouseAcc) {
			debitAccCode = WarehouseCollatAcc
		} else if creditWallet.Code == int32(WarehouseAcc) {
			creditAccCode = WarehouseCollatAcc
		} else {
			return 0, ErrInvalidCollatTx
		}

		err := CreateTransfer(ctx, q, TransferInput{
			TxId:           txId,
			DebitWalletId:  input.DebitWalletId,
			DebitAccCode:   debitAccCode,
			CreditWalletId: input.CreditWalletId,
			CreditAccCode:  creditAccCode,
			LedgerId:       asset.LedgerId,
			Amount:         asset.Amount,
			Code:           input.Code,
		})
		if err != nil {
			return 0, err
		}

		return txId, nil
	}

	// Handle normal transactions here
	for _, a := range input.Assets {
		err := CreateTransfer(ctx, q, TransferInput{
			TxId:           txId,
			DebitWalletId:  input.DebitWalletId,
			DebitAccCode:   AccountCode(debitWallet.Code),
			CreditWalletId: input.CreditWalletId,
			CreditAccCode:  AccountCode(creditWallet.Code),
			LedgerId:       a.LedgerId,
			Amount:         a.Amount,
			Code:           input.Code,
		})
		if err != nil {
			return 0, err
		}
	}

	return txId, nil
}

type TransferInput struct {
	TxId int64

	// DebitAccId    int64 // Supply AccId or WalletId

	DebitWalletId int64 // Supply AccId or WalletId
	DebitAccCode  AccountCode

	// CreditAccId    int64 // Supply AccId or WalletId

	CreditWalletId int64 // Supply AccId or WalletId
	CreditAccCode  AccountCode

	LedgerId int64
	Amount   int64
	Code     TxCode // Code for transfer
}

func CreateTransfer(ctx context.Context, q *gensql.Queries, input TransferInput) error {
	var debitAccId int64
	var creditAccId int64

	// Debit the debit wallet's accounts. If an account doesn't exist and the debit wallet is
	// a credit type account, return balance error.
	if input.DebitAccCode.IsCredit() {
		id, err := q.UpdateCreditAccountDebitsPosted(ctx, gensql.UpdateCreditAccountDebitsPostedParams{
			DebitsPosted: input.Amount,
			WalletID:     input.DebitWalletId,
			Code:         int32(input.DebitAccCode),
			LedgerID:     input.LedgerId,
		})
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrInvalidBalance
		}
		debitAccId = id
	} else {
		id, err := q.UpdateDebitAccountDebitsPosted(ctx, gensql.UpdateDebitAccountDebitsPostedParams{
			DebitsPosted: input.Amount,
			WalletID:     input.DebitWalletId,
			Code:         int32(input.DebitAccCode),
			LedgerID:     input.LedgerId,
		})
		if errors.Is(err, pgx.ErrNoRows) {
			id, err := q.InsertAccount(ctx, gensql.InsertAccountParams{
				WalletID:       input.DebitWalletId,
				DebitsPending:  0,
				DebitsPosted:   input.Amount,
				CreditsPending: 0,
				CreditsPosted:  0,
				LedgerID:       input.LedgerId,
				Code:           int32(input.DebitAccCode),
				Flags:          0,
				CreatedAt:      time.Now(),
			})
			if err != nil {
				return err
			}
			debitAccId = id
		} else {
			debitAccId = id
		}
	}
	// Credit the credit wallet's accounts. If an account doesn't exist and the credit wallet is
	// a debit type account, return balance error.
	if input.CreditAccCode.IsCredit() {
		id, err := q.UpdateCreditAccountCreditsPosted(ctx, gensql.UpdateCreditAccountCreditsPostedParams{
			CreditsPosted: input.Amount,
			WalletID:      input.CreditWalletId,
			Code:          int32(input.CreditAccCode),
			LedgerID:      input.LedgerId,
		})
		if errors.Is(err, pgx.ErrNoRows) {
			id, err := q.InsertAccount(ctx, gensql.InsertAccountParams{
				WalletID:       input.CreditWalletId,
				DebitsPending:  0,
				DebitsPosted:   0,
				CreditsPending: 0,
				CreditsPosted:  input.Amount,
				LedgerID:       input.LedgerId,
				Code:           int32(input.CreditAccCode),
				Flags:          0,
				CreatedAt:      time.Now(),
			})
			if err != nil {
				return err
			}
			creditAccId = id
		} else {
			creditAccId = id
		}
	} else {
		id, err := q.UpdateDebitAccountCreditsPosted(ctx, gensql.UpdateDebitAccountCreditsPostedParams{
			CreditsPosted: input.Amount,
			WalletID:      input.CreditWalletId,
			Code:          int32(input.CreditAccCode),
			LedgerID:      input.LedgerId,
		})
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrInvalidBalance
		}
		creditAccId = id
	}

	// Create transfer record
	_, err := q.InsertTransfer(ctx, gensql.InsertTransferParams{
		TransactionID:   input.TxId,
		DebitAccountID:  debitAccId,
		CreditAccountID: creditAccId,
		Amount:          input.Amount,
		PendingID:       nil,
		LedgerID:        input.LedgerId,
		Code:            int32(input.Code),
		Flags:           0,
		CreatedAt:       time.Now(),
	})
	return err
}
