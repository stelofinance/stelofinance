package routes

import (
	"log/slog"

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

	mux.With(midware.AuthSession(logger, sessionsKV, false)).Handle("GET /", handlers.Index(tmpls))
	mux.Handle("GET /login", handlers.Login(tmpls))

	// Login routes
	mux.Route("/auth/{provider}", func(mux chi.Router) {
		mux.Use(midware.GothicChiAdapter)

		mux.Handle("GET /", handlers.AuthStart())
		mux.Handle("GET /callback", handlers.AuthCallback(logger, db, sessionsKV))
	})

	// App related routes
	mux.Route("/app", func(mux chi.Router) {
		mux.Use(midware.AuthSession(logger, sessionsKV, true))

		mux.Handle("GET /wallets", handlers.Wallets(tmpls, db))
		mux.Handle("POST /wallets", handlers.WalletsCreate(db))
		mux.Route("/wallets/{wallet_addr}", func(mux chi.Router) {
			mux.Group(func(mux chi.Router) {
				mux.Use(midware.AuthWallet(db, accounts.PermReadBals))

				mux.Handle("GET /", handlers.WalletHome(tmpls, db))
				mux.Handle("GET /updates", handlers.WalletHomeUpdates(tmpls, db, nc))

				mux.Handle("GET /assets", handlers.WalletAssets(tmpls, db))
				mux.Handle("GET /assets/updates", handlers.WalletAssetsUpdates(tmpls, db, nc))

				mux.Handle("GET /transactions", handlers.WalletTransactions(tmpls, db))
				mux.Handle("GET /transactions/updates", handlers.WalletTransactionsUpdates(tmpls, db, nc))

				mux.Handle("GET /market", handlers.WalletMarket(tmpls, db))
			})

			mux.Group(func(mux chi.Router) {
				mux.Use(midware.AuthWallet(db, accounts.PermAdmin))

				mux.Handle("GET /transact", handlers.WalletTransact(tmpls, db))
				mux.Handle("POST /transact", handlers.WalletCreateTransaction(tmpls, db, nc))

				mux.Handle("GET /settings", handlers.WalletSettings(tmpls, db))
				mux.Handle("POST /users", handlers.WalletAddUser(tmpls, db))

				mux.Handle("DELETE /users/{discord_username}", handlers.WalletRemoveUser(tmpls, db))
				mux.Handle("GET /users/{discord_username}", handlers.WalletUserSettings(tmpls, db))
				mux.Handle("PUT /users/{discord_username}/permissions", handlers.UpdateWalletUserSettings(tmpls, db))
			})
		})

		mux.Handle("GET /warehouses", handlers.Warehouses(tmpls, db))
		mux.Handle("POST /warehouses", handlers.CreateWarehouse(tmpls, db))
		mux.Route("/warehouses/{wallet_addr}", func(mux chi.Router) {
			mux.Group(func(mux chi.Router) {
				mux.Use(midware.AuthWallet(db, accounts.PermReadBals))

				mux.Handle("GET /", handlers.WarehouseHome(tmpls, db))
			})

			mux.Group(func(mux chi.Router) {
				mux.Use(midware.AuthWallet(db, accounts.PermAdmin))

				mux.Handle("GET /", handlers.WarehouseHome(tmpls, db))
			})
		})

		mux.Handle("GET /logout", handlers.Logout(sessionsKV))
	})
}
