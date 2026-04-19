package handlers

import (
	"database/sql"
	"encoding/json"
	"errors"
	"net/http"
	"strconv"
	"time"

	"github.com/dchest/uniuri"
	"github.com/go-chi/chi/v5"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/database/gensql"
	"github.com/stelofinance/stelofinance/internal/logger"
	"github.com/stelofinance/stelofinance/internal/sessions"
)

type LoginKV struct {
	Username string `json:"username"`
	PlayerId string `json:"playerId"`
}

func Auth(lgr *logger.Logger, db *database.Database, sessionsKV jetstream.KeyValue, getenv func(string) string) http.HandlerFunc {
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
				lgr.Log(logger.Log{
					Message: "error fetching session login value",
					Data: map[string]any{
						"error": err.Error(),
					},
					Level: logger.ErrorLevel,
				})
				w.WriteHeader(http.StatusInternalServerError)
				return
			}
			if err := json.Unmarshal(val.Value(), &playerInfo); err != nil {
				lgr.Log(logger.Log{
					Message: "error unmarshalling login data",
					Data: map[string]any{
						"error": err.Error(),
					},
					Level: logger.ErrorLevel,
				})
				w.WriteHeader(http.StatusInternalServerError)
				return
			}
		}

		var userId int64

		// Check if user already exists, if not, create user
		dbUser, err := db.Q.GetUserByBitCraftId(r.Context(), playerInfo.PlayerId)
		if err != nil && !errors.Is(err, sql.ErrNoRows) {
			lgr.Log(logger.Log{
				Message: "error getting user for auth",
				Data: map[string]any{
					"error": err.Error(),
				},
				Level: logger.ErrorLevel,
			})
			w.WriteHeader(http.StatusInternalServerError)
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
				lgr.Log(logger.Log{
					Message: "error creating user",
					Data: map[string]any{
						"error": err.Error(),
					},
					Level: logger.ErrorLevel,
				})
				w.WriteHeader(http.StatusInternalServerError)
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
			lgr.Log(logger.Log{
				Message: "error marshalling session data",
				Data: map[string]any{
					"error": err.Error(),
				},
				Level: logger.ErrorLevel,
			})
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		sessionsKV.Create(r.Context(), "users."+strconv.FormatInt(userId, 10)+".sessions."+sid, bytes)

		http.SetCookie(w, &cookie)
		http.Redirect(w, r, "/app", http.StatusFound)
	}
}
