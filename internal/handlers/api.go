package handlers

import (
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/nats-io/nats.go"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/database/gensql"
	"github.com/stelofinance/stelofinance/internal/accounts"
	"github.com/stelofinance/stelofinance/internal/sessions"
)

func CreateLedger(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		type Input struct {
			Name  string `json:"name" validate:"required"`
			Scale int64  `json:"scale" validate:"min=0"`
			Code  int64  `json:"code" validate:"min=0"`
		}
		var body Input
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		if err := validate.Struct(body); err != nil {
			fmt.Println(err)
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		// TODO: Actually validate ledger codes

		_, err := db.Q.InsertLedger(r.Context(), gensql.InsertLedgerParams{
			Name:       body.Name,
			AssetScale: body.Scale,
			Code:       body.Code,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		w.WriteHeader(http.StatusCreated)
	}
}
func Ledgers(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		ldgrs, err := db.Q.GetAllLedgers(r.Context())
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		if len(ldgrs) < 1 {
			ldgrs = make([]gensql.Ledger, 0)
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

func LedgerAudit(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		ledgerId, err := strconv.ParseInt(chi.URLParam(r, "ledger_id"), 10, 64)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		audit, err := db.Q.LedgerBalanceAudit(r.Context(), ledgerId)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		data, err := json.Marshal(audit)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.Write(data)
	}
}

func User(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		userId, err := strconv.ParseInt(chi.URLParam(r, "user_id"), 10, 64)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		user, err := db.Q.GetUserById(r.Context(), userId)
		if err != nil {
			if errors.Is(err, sql.ErrNoRows) {
				w.WriteHeader(http.StatusNotFound)
				return
			}
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		data, err := json.Marshal(user)
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
		searchTerm := r.URL.Query().Get("term")
		ledgerIdStr := r.URL.Query().Get("ledgerid")
		if searchTerm == "" || ledgerIdStr == "" {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		ledgerId, err := strconv.ParseInt(ledgerIdStr, 10, 64)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		result, err := db.Q.SearchAccountsByAddrAndUsername(r.Context(), gensql.SearchAccountsByAddrAndUsernameParams{
			SearchTerm:       "%" + strings.ToUpper(searchTerm) + "%",
			ExcludeAccountID: -1,
			LedgerID:         ledgerId,
			Limit:            10,
		})
		if err != nil {
			// if errors.Is(err, sql.ErrNoRows) {
			// 	w.WriteHeader(http.StatusNotFound)
			// 	return
			// }
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		// So empty marshalls as empty array
		if len(result) == 0 {
			result = make([]gensql.SearchAccountsByAddrAndUsernameRow, 0)
		}

		data, err := json.Marshal(result)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.Write(data)
	}
}

func CreateAccount(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		type Input struct {
			Addr     string  `json:"addr"`
			Webhook  *string `json:"webhook" validate:"omitnil,url"`
			OwnerId  int64   `json:"ownerId" validate:"required"`
			LedgerId int64   `json:"ledgerId" validate:"required"`
			Code     int64   `json:"code"`
		}
		var body Input
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		if err := validate.Struct(body); err != nil {
			fmt.Println(err)
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		tx, err := db.Pool.BeginTx(r.Context(), nil)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer tx.Rollback()

		_, err = accounts.CreateAccount(r.Context(), db.Q.WithTx(tx), accounts.CreateAccountInput{
			OwnerId:  body.OwnerId,
			Address:  body.Addr,
			Webhook:  body.Webhook,
			LedgerId: body.LedgerId,
			Code:     accounts.AccountCode(body.Code),
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		tx.Commit()

		w.WriteHeader(http.StatusCreated)
	}
}
func Account(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		aData := sessions.GetAccount(r.Context())
		acc, err := db.Q.GetAccountById(r.Context(), aData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		type Response struct {
			UserID         *int64    `json:"userId"`
			Balance        int64     `json:"balance"`
			DebitsPending  int64     `json:"debitsPending"`
			DebitsPosted   int64     `json:"debitsPosted"`
			CreditsPending int64     `json:"creditsPending"`
			CreditsPosted  int64     `json:"creditsPosted"`
			LedgerID       int64     `json:"ledgerId"`
			Code           int64     `json:"code"`
			CreatedAt      time.Time `json:"createdAt"`
		}

		bal := acc.DebitsPosted - (acc.CreditsPosted + acc.CreditsPending)
		if accounts.AccountCode(acc.Code).IsCredit() {
			bal = acc.CreditsPosted - (acc.DebitsPosted + acc.DebitsPending)
		}

		rsp := Response{
			UserID:         acc.UserID,
			Balance:        bal,
			DebitsPending:  acc.DebitsPending,
			DebitsPosted:   acc.DebitsPosted,
			CreditsPending: acc.CreditsPending,
			CreditsPosted:  acc.CreditsPosted,
			LedgerID:       acc.LedgerID,
			Code:           acc.Code,
			CreatedAt:      acc.CreatedAt,
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

// TODO: Allow offset and limit query params
func Transfers(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		accData := sessions.GetAccount(r.Context())

		trs, err := db.Q.GetTransfersByAccountId(r.Context(), accData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		type ResponseRow struct {
			ID int64 `json:"id"`

			DebitAccId  int64  `json:"debitAccId"`
			CreditAccId int64  `json:"creditAccId"`
			Amount      int64  `json:"amount"`
			LedgerID    int64  `json:"ledgerId"`
			DebitAddr   string `json:"debitAddr"`
			CreditAddr  string `json:"creditAddr"`

			// PendingID       *int64    `json:"pendingId"`
			Code int32   `json:"code"`
			Memo *string `json:"memo,omitempty"`
			// Flags     uint8     `json:"flags"`
			CreatedAt time.Time `json:"createdAt"`
		}

		rsp := make([]ResponseRow, 0, len(trs))

		for _, t := range trs {
			rsp = append(rsp, ResponseRow{
				ID:          t.ID,
				DebitAccId:  t.DebitAccountID,
				CreditAccId: t.CreditAccountID,
				Amount:      t.Amount,
				LedgerID:    t.LedgerID,
				DebitAddr:   t.DebitAddress,
				CreditAddr:  t.CreditAddress,
				Code:        int32(t.Code),
				Memo:        t.Memo,
				CreatedAt:   t.CreatedAt,
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

func Transfer(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		accData := sessions.GetAccount(r.Context())

		trId := chi.URLParam(r, "tr_id")
		trIdNum, err := strconv.Atoi(trId)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		tr, err := db.Q.GetTransferWithAddrsById(r.Context(), int64(trIdNum))
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		// Don't allow reading other's transfer(s)
		if tr.CreditAccountID != accData.Id && tr.DebitAccountID != accData.Id {
			w.WriteHeader(http.StatusNotFound)
			return
		}

		type Response struct {
			ID int64 `json:"id"`

			DebitAccId  int64  `json:"debitAccId"`
			CreditAccId int64  `json:"creditAccId"`
			Amount      int64  `json:"amount"`
			LedgerID    int64  `json:"ledgerId"`
			DebitAddr   string `json:"debitAddr"`
			CreditAddr  string `json:"creditAddr"`

			// PendingID       *int64    `json:"pendingId"`
			Code int32   `json:"code"`
			Memo *string `json:"memo,omitempty"`
			// Flags     uint8     `json:"flags"`
			CreatedAt time.Time `json:"createdAt"`
		}

		rsp := Response{
			ID:          tr.ID,
			DebitAccId:  tr.DebitAccountID,
			CreditAccId: tr.CreditAccountID,
			Amount:      tr.Amount,
			LedgerID:    tr.LedgerID,
			DebitAddr:   tr.DebitAddress,
			CreditAddr:  tr.CreditAddress,
			Code:        int32(tr.Code),
			Memo:        tr.Memo,
			CreatedAt:   tr.CreatedAt,
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

func CreateTransfer(db *database.Database, nc *nats.Conn) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		accData := sessions.GetAccount(r.Context())

		type Input struct {
			ReceivingId int64   `json:"receivingId" validate:"required"`
			Memo        *string `json:"memo"`
			LedgerId    int64   `json:"ledgerId" validate:"required"`
			Amount      int64   `json:"amount" validate:"min=1"`
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

		tx, err := db.Pool.BeginTx(r.Context(), nil)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer tx.Rollback()

		_, sendEvents, err := accounts.CreateTransfer(r.Context(), db.Q.WithTx(tx), nc, accounts.CreateTransferInput{
			SendingId:   accData.Id,
			ReceivingId: body.ReceivingId,
			Memo:        body.Memo,
			LedgerId:    body.LedgerId,
			Amount:      body.Amount,
		})
		if err != nil {
			switch err {
			case accounts.ErrInvalidBalance:
				w.WriteHeader(http.StatusBadRequest)
				return
			default:
				w.WriteHeader(http.StatusInternalServerError)
				return
			}
		}

		tx.Commit()
		go sendEvents()

		w.WriteHeader(http.StatusCreated)
	}
}

func GetWebhook(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		accData := sessions.GetAccount(r.Context())

		accResult, err := db.Q.GetAccountById(r.Context(), accData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		data, err := json.Marshal(accResult.Webhook)
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
		accData := sessions.GetAccount(r.Context())

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

		err = db.Q.UpdateAccountWebhookById(r.Context(), gensql.UpdateAccountWebhookByIdParams{
			Webhook: &body.Webhook,
			ID:      accData.Id,
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
		accData := sessions.GetAccount(r.Context())

		err := db.Q.UpdateAccountWebhookById(r.Context(), gensql.UpdateAccountWebhookByIdParams{
			Webhook: nil,
			ID:      accData.Id,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		w.WriteHeader(http.StatusOK)
	}
}
