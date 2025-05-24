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

		mux.Route("/wallets/{wallet_addr}", func(mux chi.Router) {
			mux.Group(func(mux chi.Router) {
				mux.Use(midware.AuthWallet(db, accounts.PermReadBals))

				mux.Handle("GET /", handlers.WalletHome(tmpls, db))
				mux.Handle("GET /updates", handlers.WalletHomeUpdates(tmpls, db, nc))

				mux.Handle("GET /assets", handlers.WalletAssets(tmpls, db))
				mux.Handle("GET /assets/updates", handlers.WalletAssetsUpdates(tmpls, db, nc))
			})
		})

		mux.Handle("GET /logout", handlers.Logout(sessionsKV))
	})
}
