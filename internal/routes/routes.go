package routes

import (
	"net/http"

	"github.com/go-chi/chi/v5"
	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/internal/accounts"
	"github.com/stelofinance/stelofinance/internal/assets"
	"github.com/stelofinance/stelofinance/internal/handlers"
	"github.com/stelofinance/stelofinance/internal/logger"
	midware "github.com/stelofinance/stelofinance/internal/middlewares"
	"github.com/stelofinance/stelofinance/web/templates"
)

func AddRoutes(
	mux *chi.Mux,
	lgr *logger.Logger,
	tmpls *templates.Tmpls,
	db *database.Database,
	sessionsKV jetstream.KeyValue,
	nc *nats.Conn,
	getenv func(string) string,
) {
	assets.HttpHandler(mux)

	mux.Handle("GET /hotreload", handlers.HotReload())

	mux.With(midware.AuthUser(lgr, sessionsKV, false)).Handle("GET /", handlers.Index(tmpls))

	// Login/Auth routes
	// TODO: Thes routes should be guest protected
	mux.Handle("GET /login", handlers.Login(tmpls, sessionsKV))
	mux.Handle("GET /auth/{key}", handlers.Auth(lgr, db, sessionsKV, getenv))

	// App related routes
	mux.Route("/app", func(mux chi.Router) {
		mux.Use(midware.AuthUser(lgr, sessionsKV, true))

		mux.Handle("GET /", handlers.AppHome(tmpls, db))

		mux.Handle("GET /accounts", handlers.AppAccounts(tmpls, db))
		mux.Handle("GET /accounts/updates", handlers.AppAccountsUpdates(tmpls, db, nc))
		mux.Handle("POST /accounts", handlers.AppCreateAccount(tmpls, db))

		mux.Group(func(mux chi.Router) {
			mux.Use(midware.AuthUserAccount(db, accounts.PermAdmin))

			mux.Handle("GET /accounts/{account_id}", handlers.AppAccount(tmpls, db, sessionsKV))
			mux.Handle("PUT /accounts/{account_id}/user-id", handlers.PutAccountUser(tmpls, db, sessionsKV))
			mux.Handle("POST /accounts/{account_id}/users", handlers.PostAccountUser(tmpls, db, sessionsKV))
			mux.Handle("DELETE /accounts/{account_id}/users/{user_id}", handlers.DeleteAccountUser(tmpls, db, sessionsKV))
			mux.Handle("POST /accounts/{account_id}/tokens", handlers.PostAccountToken(tmpls, db, sessionsKV))
			mux.Handle("DELETE /accounts/{account_id}/tokens", handlers.DeleteAccountTokens(tmpls, db, sessionsKV))
			mux.Handle("POST /accounts/{account_id}/transfers", handlers.SubmitTransfer(tmpls, db, nc))
		})

		mux.Handle("GET /transfers", handlers.AppTransfers(tmpls, db))
		mux.Handle("GET /transfers/updates", handlers.AppTransfersUpdates(tmpls, db, nc))

		mux.Handle("GET /transfers/form-recipient", handlers.FormRecipient(tmpls, db))

		mux.Handle("GET /logout", handlers.Logout(sessionsKV))
	})

	// API related routes
	mux.Route("/api", func(mux chi.Router) {
		mux.Handle("GET /ledgers", handlers.Ledgers(db))
		mux.With(midware.AuthAdmin(getenv)).Handle("POST /ledgers", handlers.CreateLedger(db))
		mux.With(midware.AuthAdmin(getenv)).Handle("GET /ledgers/{ledger_id}/audit", handlers.LedgerAudit(db))

		// Simple no-auth ping route
		mux.Handle("GET /ping", http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			w.Write([]byte("pong"))
		}))

		mux.With(midware.AuthAdmin(getenv)).Handle("GET /users/{user_id}", handlers.User(db))

		mux.Handle("GET /accounts", handlers.Accounts(db))

		mux.With(midware.AuthAdmin(getenv)).Handle("POST /accounts", handlers.CreateAccount(db))
		mux.With(midware.AuthAdmin(getenv)).Handle("PUT /accounts/{account_id}/address", handlers.UpdateAddress(db))
		// Change balances route

		mux.Route("/accounts/{account_id}", func(mux chi.Router) {
			mux.Use(midware.AuthAccountToken(sessionsKV))

			// Simple auth'd ping route
			mux.Handle("GET /ping", http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Write([]byte("pong"))
			}))

			mux.Handle("GET /", handlers.Account(db))

			mux.Handle("GET /transfers", handlers.Transfers(db))
			mux.Handle("GET /transfers/{tr_id}", handlers.Transfer(db))
			mux.Handle("POST /transfers", handlers.CreateTransfer(db, nc))

			mux.Handle("GET /webhook", handlers.GetWebhook(db))
			mux.Handle("PUT /webhook", handlers.PutWebhook(db))
			mux.Handle("DELETE /webhook", handlers.DeleteWebhook(db))
		})

	})
}
