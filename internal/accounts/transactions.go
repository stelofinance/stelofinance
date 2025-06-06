package accounts

import (
	"context"
	"errors"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/nats-io/nats.go"
	"github.com/stelofinance/stelofinance/database/gensql"
)

type TxCode int32

const (
	// Transfers from system accounts to user accounts
	TxSysUser TxCode = iota

	// Transaction to/from warehouse collateral
	TxCollateral

	// Withdrawal from or deposit into a warehouse
	TxWarehouseTransfer

	// Transaction of user wallet to user wallet
	TxUserToUser

	// Warehouse to Warehouse transfer
	TxWarehouseToWarehouseTransfer
)

type TxStatus int32

const (
	TxPosted TxStatus = iota
	TxPending
	TxPostPending
	TxVoidPending
)

type TxInput struct {
	DebitWalletId  int64
	CreditWalletId int64
	Code           TxCode
	Memo           *string
	IsPending      bool
	Assets         []TxAssets
}

type TxAssets struct {
	LedgerId int64
	Amount   int64
}

var ErrInvalidBalance = errors.New("transaction: invalid balance")
var ErrInvalidCollatTx = errors.New("transaction: invalid collateral transaction")
var ErrInvalidWarehouseAsset = errors.New("transaction: invalid warehouse asset")
var ErrInvalidWarehouseTransfer = errors.New("transaction: invalid warehouse transaction")
var ErrInvalidUserToUser = errors.New("transaction: invalid user to user transaction")

func CreateTransaction(ctx context.Context, q *gensql.Queries, nc *nats.Conn, input TxInput) (int64, error) {
	txStatus := TxPosted
	if input.IsPending {
		txStatus = TxPending
	}
	// Create transaction record
	txId, err := q.InsertTransaction(ctx, gensql.InsertTransactionParams{
		DebitWalletID:  input.DebitWalletId,
		CreditWalletID: input.CreditWalletId,
		Code:           int32(input.Code),
		Memo:           input.Memo,
		Status:         int32(txStatus),
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
			Flags:          TrFlagNone,
			Code:           input.Code,
		})
		if err != nil {
			return 0, err
		}

		return txId, nil
	}

	// Validate any warehouse transfers are of the correct ledger codes,
	// and lock collateral needed if so.
	if input.Code == TxWarehouseTransfer {
		if debitWallet.Code != int32(WarehouseAcc) && creditWallet.Code != int32(WarehouseAcc) {
			return 0, ErrInvalidWarehouseTransfer
		}

		// Must be a pending tx
		if !input.IsPending {
			return 0, ErrInvalidWarehouseTransfer
		}

		// Create arr and map of ledger IDs
		ledgerIds := make([]int64, 0, len(input.Assets))
		for _, a := range input.Assets {
			ledgerIds = append(ledgerIds, a.LedgerId)
		}

		// query the codes
		ledgers, err := q.GetLedgers(ctx, ledgerIds)
		if err != nil {
			return 0, err
		}
		for _, ledger := range ledgers {
			if !LedgerCode(ledger.Code).isDepositable() {
				return 0, ErrInvalidWarehouseAsset
			}
		}

		// Lock collateral if needed.
		// Auto unlocking collateral isn't handled (yet?)
		if creditWallet.Code == int32(WarehouseAcc) {
			float, err := creditWallet.CollateralPercentage.Float64Value()
			if err != nil {
				return 0, err
			}
			collatRatio := float.Float64
			collatNeeded := 0.0
			for _, ledger := range ledgers {
				collatNeeded += float64(ledger.Value) * collatRatio
			}

			if collatNeeded >= 1 {
				err = CreateTransfer(ctx, q, TransferInput{
					TxId:           txId,
					DebitWalletId:  creditWallet.ID,
					DebitAccCode:   WarehouseCollatLkdAcc,
					CreditWalletId: creditWallet.ID,
					CreditAccCode:  WarehouseCollatAcc,
					LedgerId:       1,                   // TODO: Dynamically match this to Stelo ledger id
					Amount:         int64(collatNeeded), // fine to truncate instead of round?
					Flags:          TrFlagPending,
					Code:           input.Code,
				})
				if err != nil {
					return 0, err
				}
			}
		}
	}

	// Validate TxUserToUser is actually that
	if input.Code == TxUserToUser {
		if !AccountCode(debitWallet.Code).IsUserCode() || !AccountCode(creditWallet.Code).IsUserCode() {
			return 0, ErrInvalidUserToUser
		}
	}

	// Set flags
	flags := TrFlagNone
	if input.IsPending {
		flags = TrFlagPending
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
			Flags:          flags,
			Code:           input.Code,
		})
		if err != nil {
			return 0, err
		}
	}

	// TODO: Send actual transactions
	nc.Publish("wallets."+debitWallet.Address+".transactions", []byte("."))
	nc.Publish("wallets."+creditWallet.Address+".transactions", []byte("."))

	return txId, nil
}

type TransferFlag uint16

const TrFlagNone TransferFlag = 0

const (
	TrFlagPending TransferFlag = 1 << iota
	TrFlagPostPending
	TrFlagVoidPending
	TrFlagRESERVED4
	TrFlagRESERVED5
	TrFlagRESERVED6
	TrFlagRESERVED7
	TrFlagRESERVED8
	TrFlagRESERVED9
	TrFlagRESERVED10
	TrFlagRESERVED11
	TrFlagRESERVED12
	TrFlagRESERVED13
	TrFlagRESERVED14
	TrFlagRESERVED15
	TrFlagRESERVED16
)

// TODO: Maybe make a function that validates and converts TransferFlags to a uint16

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
	Flags    TransferFlag
	Code     TxCode // Code for transfer
}

func CreateTransfer(ctx context.Context, q *gensql.Queries, input TransferInput) error {
	var debitAccId int64
	var creditAccId int64

	isPending := false
	if TrFlagPending&input.Flags == TrFlagPending {
		isPending = true
	}

	// Debit the debit wallet's accounts. If an account doesn't exist and the debit wallet is
	// a credit type account, return balance error.
	if input.DebitAccCode.IsCredit() {
		if isPending {
			id, err := q.UpdateCreditAccountDebitsPending(ctx, gensql.UpdateCreditAccountDebitsPendingParams{
				DebitsPending: input.Amount,
				WalletID:      input.DebitWalletId,
				Code:          int32(input.DebitAccCode),
				LedgerID:      input.LedgerId,
			})
			if errors.Is(err, pgx.ErrNoRows) {
				return ErrInvalidBalance
			}
			debitAccId = id
		} else {
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
		}
	} else {
		if isPending {
			id, err := q.UpdateDebitAccountDebitsPending(ctx, gensql.UpdateDebitAccountDebitsPendingParams{
				DebitsPending: input.Amount,
				WalletID:      input.DebitWalletId,
				Code:          int32(input.DebitAccCode),
				LedgerID:      input.LedgerId,
			})
			if errors.Is(err, pgx.ErrNoRows) {
				id, err := q.InsertAccount(ctx, gensql.InsertAccountParams{
					WalletID:       input.DebitWalletId,
					DebitsPending:  input.Amount,
					DebitsPosted:   0,
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
	}
	// Credit the credit wallet's accounts. If an account doesn't exist and the credit wallet is
	// a debit type account, return balance error.
	if input.CreditAccCode.IsCredit() {
		if isPending {
			id, err := q.UpdateCreditAccountCreditsPending(ctx, gensql.UpdateCreditAccountCreditsPendingParams{
				CreditsPending: input.Amount,
				WalletID:       input.CreditWalletId,
				Code:           int32(input.CreditAccCode),
				LedgerID:       input.LedgerId,
			})
			if errors.Is(err, pgx.ErrNoRows) {
				id, err := q.InsertAccount(ctx, gensql.InsertAccountParams{
					WalletID:       input.CreditWalletId,
					DebitsPending:  0,
					DebitsPosted:   0,
					CreditsPending: input.Amount,
					CreditsPosted:  0,
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
		}
	} else {
		if isPending {
			id, err := q.UpdateDebitAccountCreditsPending(ctx, gensql.UpdateDebitAccountCreditsPendingParams{
				CreditsPending: input.Amount,
				WalletID:       input.CreditWalletId,
				Code:           int32(input.CreditAccCode),
				LedgerID:       input.LedgerId,
			})
			if errors.Is(err, pgx.ErrNoRows) {
				return ErrInvalidBalance
			}
			creditAccId = id
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
		Flags:           int64(input.Flags),
		CreatedAt:       time.Now(),
	})
	return err
}

type FinalizeInput struct {
	TxId   int64
	Status TxStatus
}

var ErrInvalidFinalizeStatus = errors.New("transaction: invalid tx status to finalize transaction")
var ErrTransactionNotPending = errors.New("transaction: transaction not in pending status")
var ErrUnexpectedNonPendingTransfer = errors.New("transaction: unexpected non-pending transfer")

// TODO: Send tx msg in NATS
func FinalizeTransaction(ctx context.Context, q *gensql.Queries, input FinalizeInput) error {
	if input.Status != TxPostPending && input.Status != TxVoidPending {
		return ErrInvalidFinalizeStatus
	}

	// Update the transaction status, if it's not found return error
	err := q.UpdateTransactionStatus(ctx, gensql.UpdateTransactionStatusParams{
		ID:            input.TxId,
		NewStatus:     int32(input.Status),
		CurrentStatus: int32(TxPending),
	})
	if err != nil {
		// TODO: I guess it could also just be not found (based on id)
		return ErrTransactionNotPending
	}

	trFlag := TrFlagPostPending
	if input.Status == TxVoidPending {
		trFlag = TrFlagVoidPending
	}

	// Query all the transfers, if any aren't pending, return error,
	// otherwise then create their finalization transfer and
	// update all the account balances.
	transfers, err := q.GetTransfersByTxId(ctx, input.TxId)
	if err != nil {
		return err
	}
	insertTransfers := make([]gensql.InsertTransfersParams, 0, len(transfers))
	accountUpdates := make([]gensql.UpdateAccountBalancesParams, 0, len(transfers)*2)
	for _, tr := range transfers {
		if TrFlagPending&TransferFlag(tr.Flags) != TrFlagPending {
			return ErrUnexpectedNonPendingTransfer
		}
		insertTransfers = append(insertTransfers, gensql.InsertTransfersParams{
			TransactionID:   tr.TransactionID,
			DebitAccountID:  tr.DebitAccountID,
			CreditAccountID: tr.CreditAccountID,
			Amount:          tr.Amount,
			PendingID:       &tr.ID,
			LedgerID:        tr.LedgerID,
			Code:            tr.Code,
			Flags:           int64(trFlag),
			CreatedAt:       time.Now(),
		})
		if trFlag == TrFlagPostPending {
			accountUpdates = append(accountUpdates, gensql.UpdateAccountBalancesParams{
				CreditsPosted: tr.Amount,
				ID:            tr.CreditAccountID,
			})
			accountUpdates = append(accountUpdates, gensql.UpdateAccountBalancesParams{
				DebitsPosted: tr.Amount,
				ID:           tr.DebitAccountID,
			})
		}
	}

	_, err = q.InsertTransfers(ctx, insertTransfers)
	for _, update := range accountUpdates {
		err := q.UpdateAccountBalances(ctx, update)
		if err != nil {
			return err
		}
	}

	return nil
}
