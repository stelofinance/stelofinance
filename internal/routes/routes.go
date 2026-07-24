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
)

func AddRoutes(
	mux *chi.Mux,
	lgr *logger.Logger,
	db *database.Database,
	sessionsKV jetstream.KeyValue,
	nc *nats.Conn,
	webhooks accounts.WebhookEnqueuer,
	getenv func(string) string,
) {
	assets.HttpHandler(mux)

	env := getenv("ENV")

	mux.Handle("GET /hotreload", handlers.HotReload())

	mux.With(midware.AuthUser(lgr, sessionsKV, false)).Handle("GET /", handlers.Index(env))

	// Login/Auth routes
	// TODO: These routes should be guest protected
	mux.Handle("GET /login", handlers.Login(env, sessionsKV))
	mux.Handle("GET /auth/{key}", handlers.Auth(lgr, db, sessionsKV, getenv))

	// App related routes
	mux.Route("/app", func(mux chi.Router) {
		mux.Use(midware.AuthUser(lgr, sessionsKV, true))

		mux.Handle("GET /", handlers.AppHome(env, db))

		mux.Handle("GET /accounts", handlers.AppAccounts(env, db))
		mux.Handle("GET /accounts/updates", handlers.AppAccountsUpdates(env, db, nc))
		mux.Handle("POST /accounts", handlers.AppCreateAccount(env, db))

		mux.Handle("GET /request", handlers.AppPaymentRequest(env, db, sessionsKV))
		mux.With(midware.AuthUserAccount(db, accounts.PermAdmin)).Handle("POST /request/{account_id}/transfers", handlers.PostRequest(db, nc, webhooks))

		mux.Group(func(mux chi.Router) {
			mux.Use(midware.AuthUserAccount(db, accounts.PermAdmin))

			mux.Handle("GET /accounts/{account_id}", handlers.AppAccount(env, db, sessionsKV))
			mux.Handle("PUT /accounts/{account_id}/user-id", handlers.PutAccountUser(env, db, sessionsKV))
			mux.Handle("POST /accounts/{account_id}/users", handlers.PostAccountUser(env, db, sessionsKV))
			mux.Handle("DELETE /accounts/{account_id}/users/{user_id}", handlers.DeleteAccountUser(env, db, sessionsKV))
			mux.Handle("POST /accounts/{account_id}/tokens", handlers.PostAccountToken(env, db, sessionsKV))
			mux.Handle("DELETE /accounts/{account_id}/tokens", handlers.DeleteAccountTokens(env, db, sessionsKV))
			mux.Handle("POST /accounts/{account_id}/transfers", handlers.SubmitTransfer(db, nc, webhooks))
		})

		mux.Handle("GET /transfers", handlers.AppTransfers(env, db))
		mux.Handle("GET /transfers/updates", handlers.AppTransfersUpdates(env, db, nc))

		mux.Handle("GET /transfers/form-recipient", handlers.FormRecipient(db))

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
		mux.With(midware.AuthAdmin(getenv)).Handle("PATCH /accounts/{account_id}/balance", handlers.PatchBalance(db))

		mux.Route("/accounts/{account_id}", func(mux chi.Router) {
			mux.Use(midware.AuthAccountToken(sessionsKV))

			// Simple auth'd ping route
			mux.Handle("GET /ping", http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Write([]byte("pong"))
			}))

			mux.Handle("GET /", handlers.Account(db))

			mux.Handle("GET /transfers", handlers.Transfers(db))
			mux.Handle("GET /transfers/{tr_id}", handlers.Transfer(db))
			mux.Handle("POST /transfers", handlers.CreateTransfer(db, nc, webhooks))

			mux.Handle("GET /webhook", handlers.GetWebhook(db))
			mux.Handle("PUT /webhook", handlers.PutWebhook(db))
			mux.Handle("DELETE /webhook", handlers.DeleteWebhook(db))
		})

	})
}
