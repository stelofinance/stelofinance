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
	"github.com/stelofinance/stelofinance/web/templates"
)

func Index(tmpls *templates.Tmpls) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		sData, ok := sessions.GetSession(r.Context())
		if !ok {
			sData = nil
		}
		tmplData := templates.DataLayoutPrimary{
			NavData: templates.DataComponentNav{},
			FooterData: templates.DataComponentFooter{
				Links: []templates.DataComponentFooterLink{{
					Href: "https://discord.gg/t6gM7v7V7T",
					Text: "Discord",
				}, {
					Href: "https://github.com/stelofinance",
					Text: "GitHub",
				}},
			},
			PageData: templates.DataPageHomepage{
				User: sData != nil,
				InfoCards: []templates.DataPageHomepageInfoCard{{
					Title: "Convienent in every way",
					Body:  "One of Stelo's core goals is to be a convienent way for managing all your finances. Once you've created your account the entire platform is at your fingertips.",
				}, {
					Title: "Connecting the physical to the digial",
					Body:  "Every item in the Stelo ecosystem is backed by the real asset in game. Whenever you want any of your digital goods in game, just visit a Stelo partnered warehouse and you'll receive the items from your account.",
				}, {
					Title: "Built to be built upon",
					Body:  "By leveraging Stelo's app platform you can build loan services, trading bots, tax systems, and so much more! If you're daring enough, you could even build another entire finance platform ontop.",
				}, {
					Title: "A simplistic currency",
					Body:  "The Stelo currency is a divisible, limited supply currency built into the Stelo platform. It's main purpose is to be the collateral against assets stored in Stelo partnered warehouses.",
				}, {
					Title: "A free platform",
					Body:  "Stelo's core functionality is completely free! No monthly subscription, no transactions fees on anything. Stelo will be monetized by other means if needed.",
				}, {
					Title: "A global exchange",
					Body:  "To showcase the power of the smart wallet system, Stelo will be creating a global exchange where users can sell goods to anyone, anytime, anywhere. This utility will be only just the start of the Stelo ecosystem.",
				}},
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err := tmpls.ExecuteTemplate(w, "pages/homepage", tmplData)
		if err != nil {
			panic(err)
		}
	})
}

func Login(tmpls *templates.Tmpls) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		tmplData := templates.DataLayoutPrimary{
			NavData: templates.DataComponentNav{},
			FooterData: templates.DataComponentFooter{
				Links: []templates.DataComponentFooterLink{{
					Href: "https://discord.gg/t6gM7v7V7T",
					Text: "Discord",
				}, {
					Href: "https://github.com/stelofinance",
					Text: "GitHub",
				}},
			},
			PageData: nil,
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err := tmpls.ExecuteTemplate(w, "pages/login", tmplData)
		if err != nil {
			panic(err)
		}
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
