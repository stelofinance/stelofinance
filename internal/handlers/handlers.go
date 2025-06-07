package handlers

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"math"
	"net/http"
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/dchest/uniuri"
	"github.com/dustin/go-humanize"
	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5"
	"github.com/markbates/goth/gothic"
	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
	datastar "github.com/starfederation/datastar/sdk/go"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/database/gensql"
	"github.com/stelofinance/stelofinance/internal/accounts"
	"github.com/stelofinance/stelofinance/internal/sessions"
	"github.com/stelofinance/stelofinance/web/templates"
)

func Index(tmpls *templates.Tmpls) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		sData := sessions.GetSession(r.Context())
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

			// Create their personal wallet
			id, err := accounts.CreatePersonalWallet(r.Context(), db.Q, accounts.CreatePersonalWalletInput{
				UserId: userId,
			})
			if err != nil {
				w.WriteHeader(http.StatusInternalServerError)
				logger.LogAttrs(
					r.Context(),
					slog.LevelError,
					"failed to create personal wallet",
					slog.String("error", err.Error()),
				)
				return
			}
			dbUser.WalletID = &id
		}

		// Get wallet address
		addr, err := db.Q.GetWalletAddr(r.Context(), *dbUser.WalletID)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			logger.LogAttrs(
				r.Context(),
				slog.LevelError,
				"failed to fetch wallet addr",
				slog.String("error", err.Error()),
			)
			return
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
			UserId:    userId,
			DiscordId: user.UserID,
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
		http.Redirect(w, r, "/app/wallets/"+addr, http.StatusFound)
	}
}

func App(tmpls *templates.Tmpls) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		// sData, ok := sessions.GetSession(r.Context())
		// if !ok {
		// 	sData = nil
		// }

		tmplData := templates.DataLayoutApp{
			Title:       "Home",
			Description: "Your homepage to the Stelo Finance platform.",
			// UserId:      sData.DiscordId,
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err := tmpls.ExecuteTemplate(w, "layouts/app", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func WalletHome(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		sData := sessions.GetSession(r.Context())

		wAddr := chi.URLParam(r, "wallet_addr")

		user, err := db.Q.GetUserById(r.Context(), sData.UserId)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pfp := ""
		if user.DiscordPfp != nil {
			pfp = *user.DiscordPfp
		}

		stelo, err := db.Q.GetAccountByWalletAddrAndLedgerName(
			r.Context(),
			gensql.GetAccountByWalletAddrAndLedgerNameParams{
				Address: wAddr,
				Name:    "stelo",
			})
		if err != nil && !errors.Is(err, pgx.ErrNoRows) {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		qty := stelo.DebitsPosted - stelo.CreditsPending - stelo.CreditsPosted

		tmplData := templates.DataLayoutApp{
			Title:       "Home",
			Description: "Wallet homepage",
			NavData: templates.DataComponentAppNav{
				WalletAddr:   wAddr,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				ActivePage: "home",
				WalletAddr: wAddr,
			},
			PageData: templates.DataPageWalletHomepage{
				WalletAddr: wAddr,
				SteloSummary: templates.DataComponentSteloSummary{
					FeaturedAsset:    "Stelo",
					FeaturedAssetQty: float64(qty) / math.Pow(10, float64(stelo.AssetScale)),
				},
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/wallet-home", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func WalletHomeUpdates(tmpls *templates.Tmpls, db *database.Database, nc *nats.Conn) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		// sData, ok := sessions.GetSession(r.Context())
		// if !ok {
		// 	panic("missing session")
		// }
		wAddr := chi.URLParam(r, "wallet_addr")

		sse := datastar.NewSSE(w, r)

		txChan := make(chan *nats.Msg)
		sub, err := nc.ChanSubscribe("wallets."+wAddr+".transactions", txChan)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

	loop:
		for {
			select {
			case <-txChan:
				stelo, err := db.Q.GetAccountByWalletAddrAndLedgerName(
					r.Context(),
					gensql.GetAccountByWalletAddrAndLedgerNameParams{
						Address: wAddr,
						Name:    "stelo",
					})
				if err != nil {
					w.WriteHeader(http.StatusInternalServerError)
					return
				}

				qty := stelo.DebitsPosted - stelo.CreditsPending - stelo.CreditsPosted

				buff := new(bytes.Buffer)
				err = tmpls.ExecuteTemplate(buff, "components/stelo-summary", templates.DataComponentSteloSummary{
					FeaturedAsset:    "Stelo",
					FeaturedAssetQty: float64(qty) / math.Pow(10, float64(stelo.AssetScale)),
				})
				if err != nil {
					panic(err)
				}
				sse.MergeFragments(string(buff.Bytes()))
			case <-r.Context().Done():
				sub.Unsubscribe()
				break loop
			}
		}
	}
}

func WalletAssets(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		sData := sessions.GetSession(r.Context())
		wAddr := chi.URLParam(r, "wallet_addr")

		user, err := db.Q.GetUserById(r.Context(), sData.UserId)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pfp := ""
		if user.DiscordPfp != nil {
			pfp = *user.DiscordPfp
		}

		accResult, err := db.Q.GetAccountBalancesByWalletAddr(r.Context(), wAddr)
		if err != nil && !errors.Is(err, pgx.ErrNoRows) {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		assets := make([]templates.DataComponentAssetAsset, 0, len(accResult))
		steloBal := 0.0

		for _, acc := range accResult {
			bal := acc.DebitBalance
			if accounts.AccountCode(acc.Code).IsCredit() {
				bal = acc.CreditBalance
			}
			balFmtd := float64(bal) / math.Pow(10, float64(acc.AssetScale))

			if acc.AssetName == "stelo" {
				steloBal = balFmtd
			} else {
				assets = append(assets, templates.DataComponentAssetAsset{
					Name: acc.AssetName,
					Qty:  balFmtd,
				})
			}
		}

		tmplData := templates.DataLayoutApp{
			Title:       "Assets",
			Description: "An overview of your wallet's assets",
			NavData: templates.DataComponentAppNav{
				WalletAddr:   wAddr,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				ActivePage: "assets",
				WalletAddr: wAddr,
			},
			PageData: templates.DataPageWalletAssets{
				WalletAddr: wAddr,
				SteloSummary: templates.DataComponentSteloSummary{
					FeaturedAsset:    "Stelo",
					FeaturedAssetQty: steloBal,
				},
				Assets: templates.DataComponentAssets{
					Assets: assets,
				},
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/wallet-assets", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func WalletAssetsUpdates(tmpls *templates.Tmpls, db *database.Database, nc *nats.Conn) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		// sData, ok := sessions.GetSession(r.Context())
		// if !ok {
		// 	panic("missing session")
		// }
		wAddr := chi.URLParam(r, "wallet_addr")

		sse := datastar.NewSSE(w, r)

		txChan := make(chan *nats.Msg)
		sub, err := nc.ChanSubscribe("wallets."+wAddr+".transactions", txChan)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

	loop:
		for {
			select {
			case <-txChan:
				accResult, err := db.Q.GetAccountBalancesByWalletAddr(r.Context(), wAddr)
				if err != nil && !errors.Is(err, pgx.ErrNoRows) {
					w.WriteHeader(http.StatusInternalServerError)
					return
				}

				assets := make([]templates.DataComponentAssetAsset, 0, len(accResult))
				steloBal := 0.0

				for _, acc := range accResult {
					bal := acc.DebitBalance
					if accounts.AccountCode(acc.Code).IsCredit() {
						bal = acc.CreditBalance
					}
					balFmtd := float64(bal) / math.Pow(10, float64(acc.AssetScale))

					if acc.AssetName == "stelo" {
						steloBal = balFmtd
					} else {
						assets = append(assets, templates.DataComponentAssetAsset{
							Name: acc.AssetName,
							Qty:  balFmtd,
						})
					}
				}

				buff := new(bytes.Buffer)
				err = tmpls.ExecuteTemplate(buff, "components/stelo-summary", templates.DataComponentSteloSummary{
					FeaturedAsset:    "Stelo",
					FeaturedAssetQty: steloBal,
				})
				err = tmpls.ExecuteTemplate(buff, "components/assets", templates.DataComponentAssets{
					Assets: assets,
				})
				if err != nil {
					panic(err)
				}
				sse.MergeFragments(string(buff.Bytes()))
			case <-r.Context().Done():
				sub.Unsubscribe()
				break loop
			}
		}
	}
}

func WalletTransact(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		sData := sessions.GetSession(r.Context())
		wAddr := chi.URLParam(r, "wallet_addr")

		user, err := db.Q.GetUserById(r.Context(), sData.UserId)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pfp := ""
		if user.DiscordPfp != nil {
			pfp = *user.DiscordPfp
		}

		accBals, err := db.Q.GetAccountBalancesByWalletAddr(r.Context(), wAddr)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		assets := make([]templates.DataTransactAsset, 0, len(accBals))
		for _, acc := range accBals {
			assets = append(assets, templates.DataTransactAsset{
				LedgerId: acc.LedgerID,
				Name:     acc.AssetName,
			})
		}

		tmplData := templates.DataLayoutApp{}
		if r.URL.Query().Has("datastar") {
			type input struct {
				Search string `json:"recipientSearch"`
				Tx     struct {
					Type          string `json:"type"`
					Recipient     string `json:"recipient"`
					NCoord        int    `json:"nCoord"`
					ECoord        int    `json:"eCoord"`
					WarehouseAddr string `json:"warehouseAddr"`
				} `json:"tx"`
			}
			var ds input
			err = json.Unmarshal([]byte(r.URL.Query().Get("datastar")), &ds)
			if err != nil {
				w.WriteHeader(http.StatusBadRequest)
				return
			}
			data := templates.DataPageWalletTransact{
				WalletAddr:     wAddr,
				Assets:         assets,
				OnlyRenderPage: true,
				TxType:         ds.Tx.Type,
				TxRecipient:    ds.Tx.Recipient,
				TxWarehouse:    ds.Tx.WarehouseAddr,
				TxNCoord:       ds.Tx.NCoord,
				TxECoord:       ds.Tx.ECoord,
			}
			if ds.Tx.Type == "deposit" {
				allAssets, err := db.Q.GetLedgersByCode(r.Context(), int32(accounts.Item))
				if err != nil {
					w.WriteHeader(http.StatusInternalServerError)
					return
				}
				for _, a := range allAssets {
					data.AllAssets = append(data.AllAssets, templates.DataTransactAsset{
						LedgerId: a.ID,
						Name:     a.Name,
					})
				}

				if ds.Tx.WarehouseAddr == "" {
					locations, err := db.Q.GetWalletsByLocation(r.Context(), gensql.GetWalletsByLocationParams{
						StDistance: fmt.Sprintf("POINT(%d %d)", ds.Tx.NCoord, ds.Tx.ECoord),
						Limit:      5,
					})
					if err != nil {
						w.WriteHeader(http.StatusInternalServerError)
						return
					}
					for _, l := range locations {
						str := strings.TrimPrefix(l.WarehouseCoordinates, "POINT(")
						str = strings.TrimSuffix(str, ")")
						coords := strings.Split(str, " ")
						if len(coords) != 2 {
							w.WriteHeader(http.StatusInternalServerError)
							return
						}
						data.WarehouseSuggestions = append(data.WarehouseSuggestions, templates.DataWarehouseSuggestion{
							Label:      fmt.Sprintf("N:%v E:%v (dist: %d)", coords[0], coords[1], l.Distance),
							WalletAddr: l.Address,
						})
					}
				}
			}
			if ds.Tx.Recipient == "" && ds.Search != "" {
				wallets, err := db.Q.SearchWalletAddr(r.Context(), gensql.SearchWalletAddrParams{
					Address: "%" + ds.Search + "%",
					Limit:   3,
				})
				if err != nil {
					w.WriteHeader(http.StatusInternalServerError)
					return
				}
				usrWallets, err := db.Q.SearchWalletAddrByDiscord(r.Context(), gensql.SearchWalletAddrByDiscordParams{
					DiscordUsername: "%" + ds.Search + "%",
					Limit:           3,
				})
				suggestions := make([]templates.DataRecipientSuggestion, 0, len(usrWallets)+len(wallets))
				for _, w := range usrWallets {
					suggestions = append(suggestions, templates.DataRecipientSuggestion{
						Type:       "user",
						Value:      w.DiscordUsername,
						WalletAddr: w.Address,
					})
				}
				for _, w := range wallets {
					suggestions = append(suggestions, templates.DataRecipientSuggestion{
						Type:       "wallet",
						Value:      w,
						WalletAddr: w,
					})
				}
				data.RecipientSuggestions = suggestions
			}

			sse := datastar.NewSSE(w, r)

			tmplData.PageData = data
			buff := new(bytes.Buffer)
			err = tmpls.ExecuteTemplate(buff, "pages/transact", tmplData)
			if err != nil {
				w.WriteHeader(http.StatusBadRequest)
				return
			}
			sse.MergeFragments(string(buff.Bytes()))
			return
		} else {
			tmplData = templates.DataLayoutApp{
				Title:       "Home",
				Description: "Wallet homepage",
				NavData: templates.DataComponentAppNav{
					WalletAddr:   wAddr,
					ProfileImage: pfp,
					Username:     user.DiscordUsername,
				},
				MenuData: templates.DataComponentAppMenu{
					ActivePage: "transact",
					WalletAddr: wAddr,
				},
				PageData: templates.DataPageWalletTransact{
					WalletAddr: wAddr,
					Assets:     assets,
					TxType:     "transfer",
				},
			}
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/transact", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func WalletCreateTransaction(tmpls *templates.Tmpls, db *database.Database, nc *nats.Conn) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wAddr := chi.URLParam(r, "wallet_addr")

		txType := r.FormValue("type")
		recipient := r.FormValue("recipientSelected")
		ledgerIdStr := r.FormValue("ledgerId")
		qtyStr := r.FormValue("qty")
		memoStr := r.FormValue("memo")

		qty, err := strconv.ParseFloat(qtyStr, 64)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		ledgerId, err := strconv.Atoi(ledgerIdStr)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		// Search ledger
		ledger, err := db.Q.GetLedger(r.Context(), int64(ledgerId))
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		var memo *string
		if memoStr != "" {
			memo = &memoStr
			if len(memoStr) > 100 {
				w.WriteHeader(http.StatusBadRequest)
				return
			}
		}

		// Search up wallet ids
		idRows, err := db.Q.GetWalletIdsByAddr(r.Context(), []string{wAddr, recipient})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		debitIndx := slices.IndexFunc(idRows, func(r gensql.GetWalletIdsByAddrRow) bool {
			return r.Address == recipient
		})
		creditIndx := slices.IndexFunc(idRows, func(r gensql.GetWalletIdsByAddrRow) bool {
			return r.Address == wAddr
		})
		if debitIndx == -1 || creditIndx == -1 {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		tx, err := db.Pool.Begin(r.Context())
		defer tx.Rollback(r.Context())
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		code := accounts.TxUserToUser
		pending := false
		creditId := idRows[creditIndx].ID
		debitId := idRows[debitIndx].ID
		if txType == "deposit" {
			code = accounts.TxWarehouseTransfer
			pending = true
			creditId = debitId
			debitId = idRows[creditIndx].ID
		}
		_, err = accounts.CreateTransaction(r.Context(), gensql.New(tx), nc, accounts.TxInput{
			DebitWalletId:  debitId,
			CreditWalletId: creditId,
			Code:           code,
			Memo:           memo,
			IsPending:      pending,
			Assets: []accounts.TxAssets{{
				LedgerId: int64(ledgerId),
				Amount:   int64(qty * math.Pow(10, float64(ledger.AssetScale))),
			}},
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		tx.Commit(r.Context())

		sse := datastar.NewSSE(w, r)
		if txType == "deposit" {
			sse.MergeFragments(`<button id="submit-btn" type="submit" disabled class="border border-neutral-500 text-xl w-full mt-4 py-2">CREATED!</button>`)
		} else {
			sse.MergeFragments(`<button id="submit-btn" type="submit" disabled class="border border-neutral-500 text-xl w-full mt-4 py-2">SENT!</button>`)
		}
	}
}

func WalletTransactions(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		sData := sessions.GetSession(r.Context())
		wAddr := chi.URLParam(r, "wallet_addr")

		user, err := db.Q.GetUserById(r.Context(), sData.UserId)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pfp := ""
		if user.DiscordPfp != nil {
			pfp = *user.DiscordPfp
		}
		txs := make([]templates.DataTransaction, 0)

		walletId, err := db.Q.GetWalletIdByAddr(r.Context(), wAddr)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		txsRes, err := db.Q.GetTransactionsByWalletId(r.Context(), gensql.GetTransactionsByWalletIdParams{
			DebitWalletID: walletId,
			Limit:         50,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		for _, tx := range txsRes {
			direction := "outgoing"
			recip := tx.DebitAddress
			if tx.DebitWalletID == walletId {
				direction = "incoming"
				recip = tx.CreditAddress
			}
			memo := ""
			if tx.Memo != nil {
				memo = *tx.Memo
			}

			txs = append(txs, templates.DataTransaction{
				Direction: direction,
				Recipient: recip,
				Timestamp: humanize.Time(tx.CreatedAt),
				Memo:      memo,
			})
		}

		tmplData := templates.DataLayoutApp{
			Title:       "History",
			Description: "Wallet transaction history",
			NavData: templates.DataComponentAppNav{
				WalletAddr:   wAddr,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				ActivePage: "home",
				WalletAddr: wAddr,
			},
			PageData: templates.DataPageWalletTransactions{
				WalletAddr:   wAddr,
				Transactions: txs,
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/wallet-transactions", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func WalletTransactionsUpdates(tmpls *templates.Tmpls, db *database.Database, nc *nats.Conn) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wAddr := chi.URLParam(r, "wallet_addr")
		walletId, err := db.Q.GetWalletIdByAddr(r.Context(), wAddr)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		sse := datastar.NewSSE(w, r)

		txChan := make(chan *nats.Msg)
		sub, err := nc.ChanSubscribe("wallets."+wAddr+".transactions", txChan)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

	loop:
		for {
			select {
			case <-txChan:
				txs := make([]templates.DataTransaction, 0)

				txsRes, err := db.Q.GetTransactionsByWalletId(r.Context(), gensql.GetTransactionsByWalletIdParams{
					DebitWalletID: walletId,
					Limit:         50,
				})
				if err != nil {
					w.WriteHeader(http.StatusInternalServerError)
					return
				}

				for _, tx := range txsRes {
					direction := "outgoing"
					recip := tx.DebitAddress
					if tx.DebitWalletID == walletId {
						direction = "incoming"
						recip = tx.CreditAddress
					}
					memo := ""
					if tx.Memo != nil {
						memo = *tx.Memo
					}

					txs = append(txs, templates.DataTransaction{
						Direction: direction,
						Recipient: recip,
						Timestamp: humanize.Time(tx.CreatedAt),
						Memo:      memo,
					})
				}

				tmplData := templates.DataLayoutApp{
					PageData: templates.DataPageWalletTransactions{
						WalletAddr:     wAddr,
						OnlyRenderPage: true,
						Transactions:   txs,
					},
				}

				buff := new(bytes.Buffer)
				err = tmpls.ExecuteTemplate(buff, "pages/wallet-transactions", tmplData)
				if err != nil {
					panic(err)
				}
				sse.MergeFragments(string(buff.Bytes()))
			case <-r.Context().Done():
				sub.Unsubscribe()
				break loop
			}
		}
	}
}

var hotReloadOnce sync.Once

func HotReload() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		sse := datastar.NewSSE(w, r)
		hotReloadOnce.Do(func() {
			// Refresh the client page as soon as connection
			// is established. This will occur only once
			// after the server starts.
			sse.ExecuteScript(
				"window.location.reload()",
				datastar.WithExecuteScriptRetryDuration(time.Second),
			)
		})

		// Freeze the event stream until the connection
		// is lost for any reason. This will force the client
		// to attempt to reconnect after the server reboots.
		<-r.Context().Done()
	}
}

func Logout(sessionsKV jetstream.KeyValue) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		sData := sessions.GetSession(r.Context())

		cookie, err := r.Cookie("sid")
		if err != nil {
			http.Redirect(w, r, "/", http.StatusFound)
			return
		}
		sid := strings.TrimPrefix(cookie.Value, "stl_")

		// Delete session
		sessionsKV.Delete(r.Context(), "users."+strconv.FormatInt(sData.UserId, 10)+".sessions."+sid)

		// Delete cookie
		c := &http.Cookie{
			Name:     "sid",
			Value:    "",
			Path:     "/",
			MaxAge:   -1,
			HttpOnly: true,
			Secure:   true,
			SameSite: http.SameSiteLaxMode,
		}
		http.SetCookie(w, c)

		// Redirect to homepage
		http.Redirect(w, r, "/", http.StatusFound)
	}
}
