package routes

import (
	"log/slog"

	"github.com/go-chi/chi/v5"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/internal/assets"
	"github.com/stelofinance/stelofinance/internal/handlers"
	midware "github.com/stelofinance/stelofinance/internal/middlewares"
	"github.com/stelofinance/stelofinance/web/templates"
)

func AddRoutes(mux *chi.Mux, logger *slog.Logger, tmpls *templates.Tmpls, db *database.Database, sessionsKV jetstream.KeyValue) {
	assets.HttpHandler(mux)

	mux.Handle("GET /hotreload", handlers.HotReload())

	mux.Handle("GET /", midware.Auth(logger, sessionsKV, false, handlers.Index(tmpls)))
	mux.Handle("GET /login", handlers.Login(tmpls))

	mux.Route("/auth/{provider}", func(mux chi.Router) {
		mux.Use(midware.GothicChiAdapter)

		mux.Handle("GET /", handlers.AuthStart())
		mux.Handle("GET /callback", handlers.AuthCallback(logger, db, sessionsKV))
	})

	// App related routes
	mux.Route("/app", func(mux chi.Router) {
		// TODO: Refactor to just use Auth midware here
		// mux.Use(midware.Auth())

		mux.Handle("GET /wallets/{wallet_addr}", midware.Auth(logger, sessionsKV, true, handlers.WalletHome(tmpls, db)))
		mux.Handle("GET /stream", midware.Auth(logger, sessionsKV, true, handlers.Stream(tmpls)))
		mux.Handle("GET /logout", midware.Auth(logger, sessionsKV, true, handlers.Logout(sessionsKV)))
	})
}
