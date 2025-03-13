package routes

import (
	"log/slog"

	"github.com/go-chi/chi/v5"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/internal/assets"
	"github.com/stelofinance/stelofinance/internal/handlers"
	"github.com/stelofinance/stelofinance/internal/middlewares"
	"github.com/stelofinance/stelofinance/web/templates"
)

func AddRoutes(mux *chi.Mux, logger *slog.Logger, tmpls *templates.Tmpls, db *database.Database, sessionsKV jetstream.KeyValue) {
	assets.HttpHandler(mux)

	mux.Handle("GET /", middlewares.Auth(logger, sessionsKV, false, handlers.Index(tmpls)))
	mux.Handle("GET /login", handlers.Login(tmpls))

	mux.Route("/auth/{provider}", func(mux chi.Router) {
		mux.Use(middlewares.GothicChiAdapter)

		mux.Handle("GET /", handlers.AuthStart())
		mux.Handle("GET /callback", handlers.AuthCallback(logger, db, sessionsKV))
	})

	mux.Handle("GET /app", middlewares.Auth(logger, sessionsKV, true, handlers.App(tmpls)))
}
