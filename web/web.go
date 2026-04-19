package web

import (
	"context"
	"database/sql"
	"embed"
	_ "embed"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"slices"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/Nintron27/pillow"
	"github.com/andybalholm/brotli"
	"github.com/dchest/uniuri"
	"github.com/go-chi/chi/v5"
	"github.com/go-chi/chi/v5/middleware"
	"github.com/go-chi/cors"
	"github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/database/gensql"
	"github.com/stelofinance/stelofinance/internal/handlers"
	"github.com/stelofinance/stelofinance/internal/logger"
	"github.com/stelofinance/stelofinance/internal/routes"
	"github.com/stelofinance/stelofinance/web/templates"
	_ "turso.tech/database/tursogo"
)

//go:embed templates/*/*.html.tmpl
var templatesFS embed.FS

type Config struct {
	Port         string
	ReadTimeout  time.Duration
	WriteTimeout time.Duration
}

// Run sets up all needed dependencies for the server, early returning with
// an error if one occurs.
func Run(ctx context.Context, getenv func(string) string, stdout, stderr io.Writer) error {
	// Create config
	cfg := Config{
		Port:         getenv("PORT"),
		ReadTimeout:  time.Second * 5,
		WriteTimeout: time.Second * 30,
	}
	if cfg.Port == "" {
		cfg.Port = "8080"
	}

	// Parse templates
	tmpls, err := templates.LoadTemplates(templatesFS, "templates/", ".html.tmpl", getenv)
	if err != nil {
		return err
	}

	// Start embedded NATS server
	jsDir := getenv("JS_DIR")
	if jsDir == "" {
		return errors.New("missing JS_DIR environment variable")
	}
	ns, err := pillow.Run(
		pillow.WithNATSServerOptions(&server.Options{
			JetStream: true,
			StoreDir:  jsDir,
		}),
		pillow.WithPlatformAdapter(ctx, getenv("ENV") == "prod", &pillow.FlyioHubAndSpoke{
			ClusterName:       "stelo_swarm",
			DisableClustering: true,
		}),
	)
	if err != nil {
		return err
	}

	nc, err := ns.NATSClient()
	if err != nil {
		return err
	}

	// Create lgr
	lgr := logger.NewLogger(logger.WarnLevel, nc)

	// Create bucket for sessions
	js, err := jetstream.New(nc)
	if err != nil {
		return err
	}
	sessionsKV, err := js.CreateKeyValue(ctx, jetstream.KeyValueConfig{
		Bucket:         "sessions",
		LimitMarkerTTL: time.Second * 5,
	})
	if err != nil {
		return err
	}

	// Run migrations
	err = database.RunMigrations(ctx, getenv)
	if err != nil {
		return err
	}

	go func() {
		for {
			time.Sleep(time.Second * 15)
			entries, err := os.ReadDir("data")
			if err != nil {
				lgr.Log(logger.Log{
					Message: "Error reading dir, lol",
					Data: map[string]any{
						"error": err.Error(),
					},
					Level: logger.WarnLevel,
				})
				log.Println(err.Error())
			}

			strs := make([]string, 0)
			for _, e := range entries {
				strs = append(strs, e.Name())
			}

			lgr.Log(logger.Log{
				Message: "Here are the files",
				Data: map[string]any{
					"files": strs,
				},
				Level: logger.WarnLevel,
			})
		}
	}()

	// Connect up turso db and create db struct
	dbConn, err := sql.Open("turso", getenv("TURSO_FILE"))
	if err != nil {
		return err
	}
	db := database.New(dbConn, gensql.New(dbConn))

	// Create and run server
	srv := NewServer(lgr, tmpls, db, sessionsKV, nc, getenv)
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
		lgr.Log(logger.Log{
			Message: "server started",
			Data: map[string]any{
				"PORT": httpServer.Addr,
			},
			Level: logger.InfoLevel,
		})
		if err := httpServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			fmt.Fprintf(os.Stderr, "error listening and serving: %s\n", err)
		}
	}()

	// TODO: move out to it's own module or something, gracefully shutdown too
	go func() {
		type Message struct {
			Username string `json:"username"`
			Text     string `json:"text"`
		}
		type MsgResponse struct {
			Messages []Message `json:"messages"`
		}
		type Player struct {
			EntityId string `json:"entityId"`
			Username string `json:"username"`
			// SignedIn bool `json:"signedIn"`
		}
		type PlyrResponse struct {
			Players []Player `json:"players"`
		}

		recentAuths := make([]string, 0, 32)

		prefix := "STL#"
		client := &http.Client{}

		// Long running loop fetching chat messages for logins
		for {
			req, err := http.NewRequest(http.MethodGet, "https://bitjita.com/api/chat?limit=10", nil)
			if err != nil {
				lgr.Log(logger.Log{
					Message: "error making BitJita request",
					Data: map[string]any{
						"error": err.Error(),
					},
					Level: logger.WarnLevel,
				})
				continue
			}
			if getenv("ENV") == "prod" {
				req.Header.Add("User-Agent", "SteloFinance/0.4.0")
			} else {
				req.Header.Add("User-Agent", "SteloFinance/0.4.0 (Dev)")
			}

			resp, err := client.Do(req)
			if err != nil {
				lgr.Log(logger.Log{
					Message: "error making request to BitJita",
					Data: map[string]any{
						"error": err.Error(),
					},
					Level: logger.WarnLevel,
				})
				continue
			}
			var data MsgResponse
			if err := json.NewDecoder(resp.Body).Decode(&data); err != nil {
				lgr.Log(logger.Log{
					Message: "error decoding BitJita request body",
					Data: map[string]any{
						"error": err.Error(),
					},
					Level: logger.WarnLevel,
				})
				continue
			}

			// Scan for Stelo logins, backwards so oldest counts first
			for _, msg := range slices.Backward(data.Messages) {
				cleanTxt := strings.TrimSpace(msg.Text)
				if strings.HasPrefix(cleanTxt, prefix) {
					pubCode := strings.TrimPrefix(cleanTxt, prefix)
					if slices.Contains(recentAuths, pubCode) {
						continue
					}

					// Fetch user information
					splitUsername := strings.SplitN(msg.Username, "/", 2)
					username := splitUsername[len(splitUsername)-1]
					req, err := http.NewRequest(http.MethodGet, "https://bitjita.com/api/players?q="+username, nil)
					if err != nil {
						lgr.Log(logger.Log{
							Message: "error making BitJita request for player info",
							Data: map[string]any{
								"error": err.Error(),
							},
							Level: logger.WarnLevel,
						})
						continue
					}
					if getenv("ENV") == "prod" {
						req.Header.Add("User-Agent", "SteloFinance/0.4.0")
					} else {
						req.Header.Add("User-Agent", "SteloFinance/0.4.0 (Dev)")
					}

					resp, err := client.Do(req)
					if err != nil {
						lgr.Log(logger.Log{
							Message: "error making request to BitJita for player info",
							Data: map[string]any{
								"error": err.Error(),
							},
							Level: logger.WarnLevel,
						})
						continue
					}
					var data PlyrResponse
					if err := json.NewDecoder(resp.Body).Decode(&data); err != nil {
						lgr.Log(logger.Log{
							Message: "error decoding BitJita request body for player info",
							Data: map[string]any{
								"error": err.Error(),
							},
							Level: logger.WarnLevel,
						})
						continue
					}
					var val handlers.LoginKV
					for _, plyr := range data.Players {
						if plyr.Username == username {
							val.Username = username
							val.PlayerId = plyr.EntityId
							break
						}
					}

					// Create login kv
					bytes, err := json.Marshal(val)
					if err != nil {
						// TODO: add log
						continue
					}
					secretKey := uniuri.New()
					_, err = sessionsKV.Create(ctx, "logins."+secretKey, bytes, jetstream.KeyTTL(time.Second*10))
					if err != nil {
						// TODO: add log
						continue
					}
					// Notify handler
					err = nc.Publish("login-notifications."+pubCode, []byte(secretKey))
					if err != nil {
						// TODO: add log
						continue
					}

					// Store this so it can be skipped next loop
					recentAuths = append(recentAuths, pubCode)
				}
			}

			// trim off recentAuths
			if len(recentAuths) > 10 {
				recentAuths = recentAuths[len(recentAuths)-10:]
			}

			resp.Body.Close()
			time.Sleep(time.Second)
		}
	}()

	// Handle graceful shutdown
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		sigCtx, cancel := signal.NotifyContext(ctx, os.Interrupt, syscall.SIGTERM)
		defer cancel()
		<-sigCtx.Done()
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		// TODO: Maybe gracefully shut this down? Currently it breaks/contradicts datastar's SSE
		if err := httpServer.Close(); err != nil {
			fmt.Fprintf(stderr, "error shutting down http server: %s\n", err)
		}
		if err := dbConn.Close(); err != nil {
			fmt.Fprintf(stderr, "error closing db: %s\n", err)
		}
		if err := ns.Shutdown(shutdownCtx); err != nil {
			fmt.Fprintf(stderr, "error shutting down nats server: %s\n", err)
		}
	}()
	wg.Wait()
	return nil
}

func NewServer(
	lgr *logger.Logger,
	tmpls *templates.Tmpls,
	db *database.Database,
	sessionsKV jetstream.KeyValue,
	nc *nats.Conn,
	getenv func(string) string,
) http.Handler {
	mux := chi.NewMux()

	// mux.Use(middleware.Logger)
	// mux.Use(sloghttp.New(logger))
	mux.Use(middleware.Logger)
	mux.Use(middleware.Recoverer)
	mux.Use(cors.Handler(cors.Options{
		AllowedOrigins:   []string{"*"},
		AllowedMethods:   []string{"GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"},
		AllowCredentials: true,
		MaxAge:           300,
	}))
	mux.Use(middleware.Heartbeat("/heartbeat"))
	mux.Use(Compressor(2))

	routes.AddRoutes(mux, lgr, tmpls, db, sessionsKV, nc, getenv)

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
