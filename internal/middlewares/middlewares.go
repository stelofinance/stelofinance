package middlewares

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"log/slog"
	"net/http"
	"strconv"
	"strings"

	"github.com/go-chi/chi/v5"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/database/gensql"
	"github.com/stelofinance/stelofinance/internal/accounts"
	"github.com/stelofinance/stelofinance/internal/sessions"
)

func AuthUser(logger *slog.Logger, sessionsKV jetstream.KeyValue, authRequired bool) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			cookie, err := r.Cookie("sid")
			if err != nil {
				if !authRequired {
					next.ServeHTTP(w, r)
					return
				}
				w.WriteHeader(http.StatusUnauthorized)
				return
			}

			sid, found := strings.CutPrefix(cookie.Value, "stl_")
			if !found {
				w.WriteHeader(http.StatusBadRequest)
				return
			}

			// Retrieve session data
			sVal, err := getKeyValueWithPattern(r.Context(), sessionsKV, "users.*.sessions."+sid)
			if err != nil {
				if errors.Is(err, ErrKeyNotFound) {
					w.WriteHeader(http.StatusBadRequest)
					return
				}
				w.WriteHeader(http.StatusInternalServerError)
				return
			}

			// Unmarshal data
			var usrData sessions.UserData
			if err := json.Unmarshal(sVal, &usrData); err != nil {
				w.WriteHeader(http.StatusBadRequest)
				return
			}

			// Add session data to request
			r = r.WithContext(sessions.WithUser(r.Context(), &usrData))
			next.ServeHTTP(w, r)
		})
	}
}

// AuthUser must be before AuthUserAccount on the middleware chain
// TODO: Maybe support repeated calls to AuthUserAccount? Save perms in Context?
func AuthUserAccount(db *database.Database, perms ...accounts.Permission) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			accIdStr := chi.URLParam(r, "account_id")
			sData := sessions.GetUser(r.Context())

			accId, err := strconv.Atoi(accIdStr)
			if err != nil {
				w.WriteHeader(http.StatusBadRequest)
				return
			}

			// Check if they have the wallet permissions
			permsResult, err := db.Q.GetAccountPermissions(r.Context(), gensql.GetAccountPermissionsParams{
				UserID:    sData.Id,
				AccountID: int64(accId),
			})
			if err != nil {
				if errors.Is(err, sql.ErrNoRows) {
					w.WriteHeader(http.StatusForbidden)
					return
				}
				w.WriteHeader(http.StatusInternalServerError)
				return
			}
			accountPerms := accounts.Permission(permsResult)

			// Add wallet data to session
			r = r.WithContext(sessions.WithAccount(r.Context(), &sessions.AccountData{
				Id: int64(accId),
			}))

			// Wallet admin can bypass all
			if accounts.PermAdmin&accountPerms == accounts.PermAdmin {
				next.ServeHTTP(w, r)
				return
			}

			for _, perm := range perms {
				if perm&accountPerms != perm {
					w.WriteHeader(http.StatusForbidden)
					return
				}
			}

			next.ServeHTTP(w, r)
		})
	}
}

func AuthAccountToken(sessionsKV jetstream.KeyValue) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			AuthHdr := r.Header.Get("Authorization")
			if AuthHdr == "" {
				w.WriteHeader(http.StatusUnauthorized)
				return
			}

			split := strings.Split(AuthHdr, "_")
			if len(split) != 2 {
				w.WriteHeader(http.StatusBadRequest)
				return
			}
			tType := split[0]
			tVal := split[1]

			if tType != "stla" {
				w.WriteHeader(http.StatusForbidden)
				return
			}

			// Get the account session token
			val, err := getKeyValueWithPattern(r.Context(), sessionsKV, "accounts.*.sessions."+tVal)
			if err != nil {
				if errors.Is(err, ErrKeyNotFound) {
					w.WriteHeader(http.StatusForbidden)
					return
				}
				w.WriteHeader(http.StatusInternalServerError)
				return
			}

			// Unmarhsal key value
			var accData sessions.AccountData
			if err := json.Unmarshal(val, &accData); err != nil {
				w.WriteHeader(http.StatusBadRequest)
				return
			}

			// Ensure key matches account addr in request
			accIdStr := chi.URLParam(r, "account_id")
			if accIdStr != strconv.Itoa(int(accData.Id)) {
				w.WriteHeader(http.StatusForbidden)
				return
			}

			// Add account data to session
			r = r.WithContext(sessions.WithAccount(r.Context(), &sessions.AccountData{
				Id: accData.Id,
			}))

			next.ServeHTTP(w, r)
		})
	}
}

func AuthAdmin(getenv func(string) string) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			AuthHdr := r.Header.Get("Authorization")
			if AuthHdr == "" {
				w.WriteHeader(http.StatusUnauthorized)
				return
			}

			if AuthHdr != getenv("ADMIN_KEY") {
				w.WriteHeader(http.StatusForbidden)
				return
			}

			next.ServeHTTP(w, r)
		})
	}
}

var ErrKeyNotFound = errors.New("middlewares: key not found")

// TODO: replace with ListKeysFiltered maybe?
// getKeyValueWithPattern will search a jetstream kv bucket for the first key that matches a pattern
func getKeyValueWithPattern(ctx context.Context, sessionsKV jetstream.KeyValue, pattern string) ([]byte, error) {
	watcher, err := sessionsKV.Watch(ctx, pattern, jetstream.IgnoreDeletes())
	if err != nil {
		return nil, err
	}
	defer watcher.Stop()

	for {
		select {
		case entry := <-watcher.Updates():
			if entry == nil {
				return nil, ErrKeyNotFound
			}
			return entry.Value(), nil
		case <-ctx.Done():
			return nil, ctx.Err()
		}
	}

}
