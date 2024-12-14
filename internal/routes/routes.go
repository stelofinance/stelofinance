package routes

import (
	"io"
	"net/http"

	"github.com/andybalholm/brotli"
	"github.com/go-chi/chi/v5"
	"github.com/go-chi/chi/v5/middleware"
	"github.com/go-chi/cors"
	"github.com/stelofinance/stelofinance/internal/assets"
	"github.com/stelofinance/stelofinance/web/pages"
)

func NewRouter() http.Handler {
	r := chi.NewRouter()

	// Global middleware
	r.Use(middleware.Logger)
	r.Use(cors.Handler(cors.Options{
		AllowedOrigins:   []string{"*"},
		AllowedMethods:   []string{"GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"},
		AllowCredentials: true,
		MaxAge:           300,
	}))
	r.Use(middleware.Heartbeat("/heartbeat"))
	// r.Use(middleware.AllowContentType("application/json"))

	// Setup compressor middleware, add brotli encoding
	compressor := middleware.NewCompressor(2)
	compressor.SetEncoder("br", func(w io.Writer, level int) io.Writer {
		return brotli.NewWriterV2(w, level)
	})
	r.Use(compressor.Handler)

	assets.HttpHandler(r)

	r.Get("/", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		pages.Homepage().Render(w)
	})

	return r
}
