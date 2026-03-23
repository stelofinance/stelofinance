package handlers

import (
	"database/sql"
	"encoding/json"
	"errors"
	"log/slog"
	"net/http"
	"strconv"
	"time"

	"github.com/dchest/uniuri"
	"github.com/go-chi/chi/v5"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/database/gensql"
	"github.com/stelofinance/stelofinance/internal/sessions"
)

type LoginKV struct {
	Username string `json:"username"`
	PlayerId string `json:"playerId"`
}

func Auth(logger *slog.Logger, db *database.Database, sessionsKV jetstream.KeyValue, getenv func(string) string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var playerInfo LoginKV
		// Check if it's an admin bypass, otherwise handle normally
		if r.URL.Query().Get("adminkey") == getenv("ADMIN_KEY") {
			playerInfo.PlayerId = r.URL.Query().Get("playerid")
			playerInfo.Username = r.URL.Query().Get("username")
		} else {
			// Retrieve the session
			scrtKey := chi.URLParam(r, "key")

			val, err := sessionsKV.Get(r.Context(), "logins."+scrtKey)
			if err != nil {
				if errors.Is(err, jetstream.ErrKeyNotFound) {
					w.WriteHeader(http.StatusNotFound)
					return
				}
				w.WriteHeader(http.StatusInternalServerError)
				return
			}
			if err := json.Unmarshal(val.Value(), &playerInfo); err != nil {
				w.WriteHeader(http.StatusInternalServerError)
				return
			}
		}

		var userId int64

		// Check if user already exists, if not, create user
		dbUser, err := db.Q.GetUserByBitCraftId(r.Context(), playerInfo.PlayerId)
		if err != nil && !errors.Is(err, sql.ErrNoRows) {
			w.WriteHeader(http.StatusInternalServerError)
			logger.LogAttrs(
				r.Context(),
				slog.LevelError,
				"failed to fetch user from db",
				slog.String("error", err.Error()),
			)
			return
		}
		userId = dbUser.ID
		if errors.Is(sql.ErrNoRows, err) {
			insertedId, err := db.Q.InsertUser(r.Context(), gensql.InsertUserParams{
				BitcraftUsername: playerInfo.Username,
				BitcraftID:       playerInfo.PlayerId,
				CreatedAt:        time.Now(),
			})
			if err != nil {
				w.WriteHeader(http.StatusInternalServerError)
				logger.LogAttrs(
					r.Context(),
					slog.LevelError,
					"failed to insert new user",
					slog.String("error", err.Error()),
				)
				return
			}
			userId = insertedId
		}

		// Create session and respond with cookie
		sid := uniuri.NewLen(28)
		cookie := http.Cookie{
			Name:     "sid",
			Value:    "stl_" + sid,
			Path:     "/",
			MaxAge:   86400 * 30,
			HttpOnly: true,
			Secure:   true,
			SameSite: http.SameSiteLaxMode,
		}
		sData := sessions.UserData{
			Id:               userId,
			BitcraftId:       playerInfo.PlayerId,
			BitCraftUsername: playerInfo.Username,
		}
		bytes, err := json.Marshal(sData)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			logger.LogAttrs(
				r.Context(),
				slog.LevelError,
				"failed to marshall session data",
				slog.String("error", err.Error()),
			)
			return
		}
		sessionsKV.Create(r.Context(), "users."+strconv.FormatInt(userId, 10)+".sessions."+sid, bytes)

		http.SetCookie(w, &cookie)
		http.Redirect(w, r, "/app", http.StatusFound)
	}
}
