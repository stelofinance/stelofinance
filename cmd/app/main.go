package main

import (
	"context"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/stelofinance/stelofinance/internal/routes"
)

func main() {
	sigCtx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel() // never fires?

	srv := &http.Server{}

	go func() {
		if err := start(sigCtx, srv); err != nil && err != http.ErrServerClosed {
			log.Fatal("Error starting server", "err", err.Error())
		}
	}()

	<-sigCtx.Done()
	log.Println("Stopping server...")

	ctx, cancel := context.WithTimeout(context.Background(), time.Second*5)
	defer cancel()

	if err := srv.Shutdown(ctx); err != nil {
		log.Println("Server shutdown failed", "err", err.Error())
	}
}

func start(ctx context.Context, srv *http.Server) error {
	r := chi.NewRouter()
	r.Mount("/", routes.NewRouter())

	portEnv := os.Getenv("PORT")
	if portEnv == "" {
		portEnv = "8080"
	}

	srv.Addr = ":" + portEnv
	srv.Handler = r
	srv.ReadTimeout = time.Second * 5
	srv.WriteTimeout = time.Second * 30
	srv.BaseContext = func(l net.Listener) context.Context {
		return ctx
	}

	log.Println("Starting server", "port", portEnv)
	return srv.ListenAndServe()
}
