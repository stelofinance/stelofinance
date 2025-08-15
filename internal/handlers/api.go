package handlers

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"strconv"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5"
	"github.com/nats-io/nats.go"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/database/gensql"
	"github.com/stelofinance/stelofinance/internal/accounts"
	"github.com/stelofinance/stelofinance/internal/sessions"
)

func Ledgers(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		ldgrs, err := db.Q.GetAllLedgers(r.Context())
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		data, err := json.Marshal(ldgrs)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.Write(data)
	}
}

func Accounts(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())
		accs, err := db.Q.GetAccountBalancesByWalletAddr(r.Context(), wData.Address)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		type ResponseRow struct {
			AssetName string `json:"assetName"`
			Balace    int64  `json:"balance"`
			LedgerID  int64  `json:"ledgerId"`
			Code      int32  `json:"code"`
		}

		rsp := make([]ResponseRow, 0, len(accs))

		for _, a := range accs {
			bal := a.DebitBalance
			if accounts.AccountCode(a.Code).IsCredit() {
				bal = a.CreditBalance
			}

			rsp = append(rsp, ResponseRow{
				AssetName: a.AssetName,
				Balace:    bal,
				LedgerID:  a.LedgerID,
				Code:      a.Code,
			})
		}

		data, err := json.Marshal(rsp)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.Write(data)
	}
}

func Transactions(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())

		txs, err := db.Q.GetTransactionsByWalletId(r.Context(), gensql.GetTransactionsByWalletIdParams{
			DebitWalletID: wData.Id,
			Limit:         50,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		type ResponseRow struct {
			ID         int64     `json:"id"`
			DebitAddr  string    `json:"debitAddr"`
			CreditAddr string    `json:"creditAddr"`
			Code       int32     `json:"code"`
			Memo       *string   `json:"memo,omitempty"`
			CreatedAt  time.Time `json:"createdAt"`
			Status     int32     `json:"status"`
		}

		rsp := make([]ResponseRow, 0, len(txs))

		for _, t := range txs {
			rsp = append(rsp, ResponseRow{
				ID:         t.ID,
				DebitAddr:  t.DebitAddress,
				CreditAddr: t.CreditAddress,
				Code:       t.Code,
				Memo:       t.Memo,
				CreatedAt:  t.CreatedAt,
				Status:     t.Status,
			})
		}

		data, err := json.Marshal(rsp)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.Write(data)
	}
}

func Transaction(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())

		txId := chi.URLParam(r, "tx_id")
		txIdNum, err := strconv.Atoi(txId)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		tx, err := db.Q.GetTransactionAndAddresses(r.Context(), gensql.GetTransactionAndAddressesParams{
			TransactionID: int64(txIdNum),
			WalletID:      wData.Id,
		})
		if err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				w.WriteHeader(http.StatusNotFound)
				return
			}
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		type Response struct {
			ID            int64     `json:"id"`
			DebitAddress  string    `json:"debitAddress"`
			CreditAddress string    `json:"creditAddress"`
			Code          int32     `json:"code"`
			Memo          *string   `json:"memo,omitempty"`
			CreatedAt     time.Time `json:"createdAt"`
			Status        int32     `json:"status"`
		}

		data, err := json.Marshal(Response{
			ID:            tx.ID,
			DebitAddress:  tx.DebitAddress,
			CreditAddress: tx.CreditAddress,
			Code:          tx.Code,
			Memo:          tx.Memo,
			CreatedAt:     tx.CreatedAt,
			Status:        tx.Status,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.Write(data)
	}
}

func CreateTransaction(db *database.Database, nc *nats.Conn) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())

		type Input struct {
			ReceivingAddr string  `json:"receivingAddr" validate:"required"`
			Memo          *string `json:"memo"`
			Transfers     []struct {
				LedgerId int64 `json:"ledgerId"`
				Amount   int64 `json:"amount" validate:"min=1"`
			} `json:"transfers" validate:"gt=0,dive"`
		}
		var body Input
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		if validate.Struct(body) != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		tx, err := db.Pool.Begin(r.Context())
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer tx.Rollback(r.Context())
		qtx := db.Q.WithTx(tx)

		// Create assets transfer array
		assetTfs := make([]accounts.TxAssets, 0, len(body.Transfers))
		for _, t := range body.Transfers {
			assetTfs = append(assetTfs, accounts.TxAssets{
				LedgerId: t.LedgerId,
				Amount:   t.Amount,
			})
		}

		_, err = accounts.CreateTransactionByAddrs(r.Context(), qtx, nc, accounts.TxByAddrsInput{
			SendingAddr:   wData.Address,
			ReceivingAddr: body.ReceivingAddr,
			Memo:          body.Memo,
			Assets:        assetTfs,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		tx.Commit(r.Context())

		w.WriteHeader(http.StatusCreated)
	}
}

func Transfers(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())

		// First, verify transaction belongs to them
		txId := chi.URLParam(r, "tx_id")
		txIdNum, err := strconv.Atoi(txId)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		_, err = db.Q.GetTransactionAndAddresses(r.Context(), gensql.GetTransactionAndAddressesParams{
			TransactionID: int64(txIdNum),
			WalletID:      wData.Id,
		})
		if err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				w.WriteHeader(http.StatusNotFound)
				return
			}
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		transfers, err := db.Q.GetTransfersByTxId(r.Context(), int64(txIdNum))
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		type ResponseRow struct {
			Amount    int64     `json:"amount"`
			LedgerID  int64     `json:"ledgerId"`
			CreatedAt time.Time `json:"createdAt"`
		}

		rsp := make([]ResponseRow, 0, len(transfers))

		for _, t := range transfers {
			rsp = append(rsp, ResponseRow{
				Amount:    t.Amount,
				LedgerID:  t.LedgerID,
				CreatedAt: t.CreatedAt,
			})
		}

		data, err := json.Marshal(rsp)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.Write(data)
	}
}

func GetWebhook(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())

		webhook, err := db.Q.GetWalletWebhookByAddr(r.Context(), wData.Address)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		data, err := json.Marshal(webhook)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.Write(data)
	}
}

func PutWebhook(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())

		type Input struct {
			Webhook string `json:"webhook"`
		}
		var body Input
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		// Verify webhook is URL
		_, err := url.ParseRequestURI(body.Webhook)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		err = db.Q.UpdateWalletWebhookByAddr(r.Context(), gensql.UpdateWalletWebhookByAddrParams{
			Webhook: &body.Webhook,
			Address: wData.Address,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		w.WriteHeader(http.StatusOK)
	}
}

func DeleteWebhook(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())

		err := db.Q.UpdateWalletWebhookByAddr(r.Context(), gensql.UpdateWalletWebhookByAddrParams{
			Webhook: nil,
			Address: wData.Address,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		w.WriteHeader(http.StatusOK)
	}
}
