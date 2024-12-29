package routes

import (
	"log/slog"

	"github.com/go-chi/chi/v5"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/internal/assets"
	"github.com/stelofinance/stelofinance/internal/handlers"
	"github.com/stelofinance/stelofinance/internal/middlewares"
)

func AddRoutes(mux *chi.Mux, logger *slog.Logger, db *database.Database, sessionsKV jetstream.KeyValue) {
	assets.HttpHandler(mux)

	mux.Handle("GET /", middlewares.Auth(logger, sessionsKV, false, handlers.Index()))
	mux.Handle("GET /login", handlers.Login())

	mux.Route("/auth/{provider}", func(mux chi.Router) {
		mux.Use(middlewares.GothicChiAdapter)

		mux.Handle("GET /", handlers.AuthStart())
		mux.Handle("GET /callback", handlers.AuthCallback(logger, db, sessionsKV))
	})
}
