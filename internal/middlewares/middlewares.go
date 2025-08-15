package middlewares

import (
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"net/http"
	"strings"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/database/gensql"
	"github.com/stelofinance/stelofinance/internal/accounts"
	"github.com/stelofinance/stelofinance/internal/sessions"
)

func GothicChiAdapter(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		provider := chi.URLParam(r, "provider")
		r = r.WithContext(context.WithValue(r.Context(), "provider", provider))

		next.ServeHTTP(w, r)
	})
}

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

// AuthUser must be before AuthUserWallet on the middleware chain
// TODO: Maybe support repeated calls to AuthUserWallet? Save perms in Context?
func AuthUserWallet(db *database.Database, perms ...accounts.Permission) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			wAddr := chi.URLParam(r, "wallet_addr")
			sData := sessions.GetUser(r.Context())

			// Check if they have the wallet permissions
			permsResult, err := db.Q.GetWalletPermissions(r.Context(), gensql.GetWalletPermissionsParams{
				ID:      sData.Id,
				Address: wAddr,
			})
			if err != nil {
				if errors.Is(err, pgx.ErrNoRows) {
					w.WriteHeader(http.StatusForbidden)
					return
				}
				w.WriteHeader(http.StatusInternalServerError)
				return
			}
			walletPerms := accounts.Permission(permsResult.Permissions)

			// Add wallet data to session
			r = r.WithContext(sessions.WithWallet(r.Context(), &sessions.WalletData{
				Id:      permsResult.WalletID,
				Address: wAddr,
			}))

			// Wallet admin can bypass all
			if accounts.PermAdmin&walletPerms == accounts.PermAdmin {
				next.ServeHTTP(w, r)
				return
			}

			for _, perm := range perms {
				if perm&walletPerms != perm {
					w.WriteHeader(http.StatusForbidden)
					return
				}
			}

			next.ServeHTTP(w, r)
		})
	}
}

func AuthWalletToken(sessionsKV jetstream.KeyValue) func(http.Handler) http.Handler {
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

			if tType != "stlw" {
				w.WriteHeader(http.StatusForbidden)
				return
			}

			// Get the wallet session token
			val, err := getKeyValueWithPattern(r.Context(), sessionsKV, "wallets.*.sessions."+tVal)
			if err != nil {
				if errors.Is(err, ErrKeyNotFound) {
					w.WriteHeader(http.StatusForbidden)
					return
				}
				w.WriteHeader(http.StatusInternalServerError)
				return
			}

			// Unmarhsal key value
			var wltData sessions.WalletData
			if err := json.Unmarshal(val, &wltData); err != nil {
				w.WriteHeader(http.StatusBadRequest)
				return
			}

			// Ensure key matches wallet addr in request
			wAddr := chi.URLParam(r, "wallet_addr")
			if wAddr != wltData.Address {
				w.WriteHeader(http.StatusForbidden)
				return
			}

			// Add wallet data to session
			r = r.WithContext(sessions.WithWallet(r.Context(), &sessions.WalletData{
				Id:      wltData.Id,
				Address: wltData.Address,
			}))

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
