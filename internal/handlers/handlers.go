package handlers

import (
	"errors"
	"log/slog"
	"net/http"
	"strconv"
	"time"

	"github.com/dchest/uniuri"
	"github.com/fxamacker/cbor/v2"
	"github.com/jackc/pgx/v5"
	"github.com/markbates/goth/gothic"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/database/gensql"
	"github.com/stelofinance/stelofinance/internal/sessions"
	"github.com/stelofinance/stelofinance/web/pages"
)

func Index() http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		sData, ok := sessions.GetSession(r.Context())
		if !ok {
			sData = nil
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		pages.Homepage(sData != nil).Render(w)
	})
}

func Login() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		pages.Login().Render(w)
	}
}

func AuthStart() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		gothic.BeginAuthHandler(w, r)
	}
}

func AuthCallback(logger *slog.Logger, db *database.Database, sessionsKV jetstream.KeyValue) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		user, err := gothic.CompleteUserAuth(w, r)
		if err != nil {
			w.WriteHeader(http.StatusUnauthorized)
			logger.LogAttrs(
				r.Context(),
				slog.LevelError,
				"failed to complete user auth",
				slog.String("error", err.Error()),
			)
			return
		}

		var userId int64 = 0

		// Check if user exists, if not, create user
		dbUser, err := db.Q.GetUser(r.Context(), user.UserID)
		if err != nil && !errors.Is(err, pgx.ErrNoRows) {
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
		if errors.Is(pgx.ErrNoRows, err) {
			insertedId, err := db.Q.InsertUser(r.Context(), gensql.InsertUserParams{
				DiscordID:       user.UserID,
				DiscordUsername: user.Name,
				DiscordPfp:      &user.AvatarURL,
				CreatedAt:       time.Now(),
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
		sid := uniuri.NewLen(24)
		cookie := http.Cookie{
			Name:     "sid",
			Value:    sid,
			Path:     "/",
			MaxAge:   86400 * 30,
			HttpOnly: true,
			Secure:   true,
			SameSite: http.SameSiteLaxMode,
		}
		sData := sessions.Data{
			UserId: userId,
		}
		bytes, err := cbor.Marshal(sData)
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
		http.Redirect(w, r, "/", http.StatusFound)
	}
}
