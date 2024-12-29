package server

import (
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"sync"
	"syscall"
	"time"

	"github.com/Nintron27/nats-pillow/pillow"
	"github.com/andybalholm/brotli"
	"github.com/go-chi/chi/v5"
	"github.com/go-chi/chi/v5/middleware"
	"github.com/go-chi/cors"
	"github.com/gorilla/sessions"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/markbates/goth"
	"github.com/markbates/goth/gothic"
	"github.com/markbates/goth/providers/discord"
	"github.com/nats-io/nats.go/jetstream"
	sloghttp "github.com/samber/slog-http"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/database/gensql"
	"github.com/stelofinance/stelofinance/internal/routes"
)

type Config struct {
	Port         string
	ReadTimeout  time.Duration
	WriteTimeout time.Duration
}

// Run sets up all needed dependencies for the server, early returning with
// an error if one occurs.
func Run(ctx context.Context, getenv func(string) string, stdout, stderr io.Writer) error {
	ctx, cancel := signal.NotifyContext(ctx, os.Interrupt, syscall.SIGTERM)
	defer cancel()

	// Create logger
	logger := slog.New(slog.NewJSONHandler(stdout, nil))

	// Create config
	cfg := Config{
		Port:         getenv("PORT"),
		ReadTimeout:  time.Second * 5,
		WriteTimeout: time.Second * 30,
	}
	if cfg.Port == "" {
		cfg.Port = "8080"
	}

	// GOTH stuff
	discordClientId := getenv("DISCORD_CLIENT_ID")
	discordClientSecret := getenv("DISCORD_CLIENT_SECRET")
	if discordClientId == "" || discordClientSecret == "" {
		return errors.New("missing DISCORD_CLIENT_ID and/or DISCORD_CLIENT_SECRET environment variables")
	}

	gothKey := getenv("GOTH_KEY")
	if gothKey == "" {
		return errors.New("missing GOTH_KEY environment variable")
	}
	key, err := base64.URLEncoding.DecodeString(gothKey)
	if err != nil {
		return err
	}
	store := sessions.NewCookieStore(key)
	store.Options.Path = "/"
	store.Options.HttpOnly = true
	store.Options.MaxAge = int(time.Hour) * 24
	store.Options.Secure = getenv("ENV") == "prod"
	store.Options.SameSite = http.SameSiteLaxMode
	gothic.Store = store
	goth.UseProviders(
		discord.New(discordClientId, discordClientSecret, "http://localhost:8080/auth/discord/callback", discord.ScopeIdentify),
	)

	// Start embedded NATS server
	nc, ns, err := pillow.Run(
		pillow.WithJetStream("tmp/js"),
		pillow.WithInProcessClient(true),
		pillow.AdapterFlyio(getenv("ENV") == "prod", pillow.FlyioOptions{
			ClusterName: "stelofinance-fly",
		}),
	)
	if err != nil {
		return err
	}

	// Create bucket for sessions
	js, err := jetstream.New(nc)
	if err != nil {
		return err
	}
	sessionsKV, err := js.CreateKeyValue(ctx, jetstream.KeyValueConfig{
		Bucket: "sessions",
	})
	if err != nil {
		return err
	}

	// Connect pgx and create db struct
	pgPool, err := pgxpool.New(ctx, getenv("POSTGRES_URI"))
	if err != nil {
		return err
	}
	db := &database.Database{
		Pool: pgPool,
		Q:    gensql.New(pgPool),
	}

	// Create and run server
	srv := NewServer(logger, db, sessionsKV)
	httpServer := &http.Server{
		Addr:         ":" + cfg.Port,
		Handler:      srv,
		ReadTimeout:  cfg.ReadTimeout,
		WriteTimeout: cfg.WriteTimeout,
		BaseContext: func(l net.Listener) context.Context {
			return ctx
		},
	}
	go func() {
		logger.LogAttrs(
			ctx,
			slog.LevelInfo,
			"server started",
			slog.String("PORT", httpServer.Addr),
		)
		if err := httpServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			fmt.Fprintf(os.Stderr, "error listening and serving: %s\n", err)
		}
	}()

	// Handle graceful shutdown
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		<-ctx.Done()
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		if err := httpServer.Shutdown(shutdownCtx); err != nil {
			fmt.Fprintf(stderr, "error shutting down http server: %s\n", err)
		}
		if err := ns.Shutdown(shutdownCtx); err != nil {
			fmt.Fprintf(stderr, "error shutting down nats server: %s\n", err)
		}
	}()
	wg.Wait()
	return nil
}

func NewServer(logger *slog.Logger, db *database.Database, sessionsKV jetstream.KeyValue) http.Handler {
	mux := chi.NewMux()

	// mux.Use(middleware.Logger)
	mux.Use(sloghttp.New(logger))
	mux.Use(middleware.Recoverer)
	mux.Use(cors.Handler(cors.Options{
		AllowedOrigins:   []string{"*"},
		AllowedMethods:   []string{"GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"},
		AllowCredentials: true,
		MaxAge:           300,
	}))
	mux.Use(middleware.Heartbeat("/heartbeat"))
	mux.Use(Compressor(2))

	routes.AddRoutes(mux, logger, db, sessionsKV)

	return mux
}

// Compress is an adapter middleware from Chi that compresses
// the response body of a given content types to a data format based
// on Accept-Encoding request header. Adapted to include Brotli encoding.
//
// NOTE: make sure to set the Content-Type header on your response
// otherwise this middleware will not compress the response body. For ex, in
// your handler you should set w.Header().Set("Content-Type", http.DetectContentType(yourBody))
// or set it manually.
//
// Passing a compression level of 2-5 is sensible value.
func Compressor(level int) func(next http.Handler) http.Handler {
	compressor := middleware.NewCompressor(level)
	compressor.SetEncoder("br", func(w io.Writer, level int) io.Writer {
		return brotli.NewWriterV2(w, level)
	})

	return compressor.Handler
}