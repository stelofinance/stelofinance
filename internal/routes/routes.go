package routes

import (
	"log/slog"
	"net/http"

	"github.com/go-chi/chi/v5"
	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/internal/accounts"
	"github.com/stelofinance/stelofinance/internal/assets"
	"github.com/stelofinance/stelofinance/internal/handlers"
	midware "github.com/stelofinance/stelofinance/internal/middlewares"
	"github.com/stelofinance/stelofinance/web/templates"
)

func AddRoutes(mux *chi.Mux, logger *slog.Logger, tmpls *templates.Tmpls, db *database.Database, sessionsKV jetstream.KeyValue, nc *nats.Conn) {
	assets.HttpHandler(mux)

	mux.Handle("GET /hotreload", handlers.HotReload())

	mux.With(midware.AuthUser(logger, sessionsKV, false)).Handle("GET /", handlers.Index(tmpls))
	mux.Handle("GET /login", handlers.Login(tmpls))

	// Login routes
	mux.Route("/auth/{provider}", func(mux chi.Router) {
		mux.Use(midware.GothicChiAdapter)

		mux.Handle("GET /", handlers.AuthStart())
		mux.Handle("GET /callback", handlers.AuthCallback(logger, db, sessionsKV))
	})

	// App related routes
	mux.Route("/app", func(mux chi.Router) {
		mux.Use(midware.AuthUser(logger, sessionsKV, true))

		mux.Handle("GET /wallets", handlers.Wallets(tmpls, db))
		mux.Handle("POST /wallets", handlers.WalletsCreate(db))
		mux.Route("/wallets/{wallet_addr}", func(mux chi.Router) {
			mux.Group(func(mux chi.Router) {
				mux.Use(midware.AuthUserWallet(db, accounts.PermReadBals))

				mux.Handle("GET /", handlers.WalletHome(tmpls, db))
				mux.Handle("GET /updates", handlers.WalletHomeUpdates(tmpls, db, nc))

				mux.Handle("GET /assets", handlers.WalletAssets(tmpls, db))
				mux.Handle("GET /assets/updates", handlers.WalletAssetsUpdates(tmpls, db, nc))

				mux.Handle("GET /transactions", handlers.WalletTransactions(tmpls, db))
				mux.Handle("GET /transactions/updates", handlers.WalletTransactionsUpdates(tmpls, db, nc))

				mux.Handle("GET /market", handlers.WalletMarket(tmpls, db))
			})

			mux.Group(func(mux chi.Router) {
				mux.Use(midware.AuthUserWallet(db, accounts.PermAdmin))

				mux.Handle("GET /transact", handlers.WalletTransact(tmpls, db))
				mux.Handle("POST /transact", handlers.WalletCreateTransaction(tmpls, db, nc))
				mux.Handle("POST /withdraws/{withdraw_tx_id}/approve", handlers.WalletApproveWithdraw(tmpls, db, nc))

				mux.Handle("POST /market/coinswap", handlers.ExecuteCoinSwap(tmpls, db, nc))

				mux.Handle("GET /settings", handlers.WalletSettings(tmpls, db, sessionsKV))
				mux.Handle("POST /users", handlers.WalletAddUser(tmpls, db))
				mux.Handle("POST /tokens", handlers.WalletCreateToken(tmpls, db, sessionsKV))
				mux.Handle("DELETE /tokens", handlers.WalletDeleteTokens(tmpls, db, sessionsKV))

				mux.Handle("DELETE /users/{discord_username}", handlers.WalletRemoveUser(tmpls, db))
				mux.Handle("GET /users/{discord_username}", handlers.WalletUserSettings(tmpls, db))
				mux.Handle("PUT /users/{discord_username}/permissions", handlers.UpdateWalletUserSettings(tmpls, db))
			})
		})

		mux.Handle("GET /warehouses", handlers.Warehouses(tmpls, db))
		mux.Handle("POST /warehouses", handlers.CreateWarehouse(tmpls, db))
		mux.Route("/warehouses/{wallet_addr}", func(mux chi.Router) {
			mux.Group(func(mux chi.Router) {
				mux.Use(midware.AuthUserWallet(db, accounts.PermReadBals))

				mux.Handle("GET /", handlers.WarehouseHome(tmpls, db))
			})

			mux.Group(func(mux chi.Router) {
				mux.Use(midware.AuthUserWallet(db, accounts.PermAdmin))

				mux.Handle("GET /deposit-withdraw", handlers.WarehouseDepositWithdraw(tmpls, db))
				mux.Handle("POST /deposits/{deposit_tx_id}/approve", handlers.ApproveDeposit(tmpls, db))
				mux.Handle("POST /deposit-withdraw", handlers.CreateWithdraw(tmpls, db, nc))
			})
		})

		mux.Handle("GET /logout", handlers.Logout(sessionsKV))
	})

	// API related routes
	mux.Route("/api", func(mux chi.Router) {
		mux.Handle("GET /ledgers", handlers.Ledgers(db))

		mux.Route("/wallets/{wallet_addr}", func(mux chi.Router) {
			mux.Use(midware.AuthWalletToken(sessionsKV))

			mux.Handle("GET /ping", http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Write([]byte("pong"))
			}))

			mux.Handle("GET /accounts", handlers.Accounts(db))

			mux.Handle("GET /transactions", handlers.Transactions(db))
			mux.Handle("GET /transactions/{tx_id}", handlers.Transaction(db))
			mux.Handle("GET /transactions/{tx_id}/transfers", handlers.Transfers(db))
			mux.Handle("POST /transactions", handlers.CreateTransaction(db, nc))

			mux.Handle("GET /webhook", handlers.GetWebhook(db))
			mux.Handle("PUT /webhook", handlers.PutWebhook(db))
			mux.Handle("DELETE /webhook", handlers.DeleteWebhook(db))
		})

	})
}
