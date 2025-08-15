package handlers

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"net/http"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/dchest/uniuri"
	"github.com/dustin/go-humanize"
	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5"
	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
	datastar "github.com/starfederation/datastar/sdk/go"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/database/gensql"
	"github.com/stelofinance/stelofinance/internal/accounts"
	"github.com/stelofinance/stelofinance/internal/sessions"
	"github.com/stelofinance/stelofinance/web/templates"
)

func WalletHome(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		wData := sessions.GetWallet(r.Context())

		user, err := db.Q.GetUserById(r.Context(), uData.Id)
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
				Address: wData.Address,
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
				WalletAddr:   wData.Address,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				ActivePage: "home",
				WalletAddr: wData.Address,
			},
			PageData: templates.DataPageWalletHomepage{
				WalletAddr: wData.Address,
				SteloSummary: templates.DataComponentSteloSummary{
					FeaturedAsset:    "Stelo",
					FeaturedAssetQty: humanize.Commaf(float64(qty) / math.Pow(10, float64(stelo.AssetScale))),
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
		// sData := sessions.GetSession(r.Context())
		wData := sessions.GetWallet(r.Context())

		sse := datastar.NewSSE(w, r)

		txChan := make(chan *nats.Msg)
		sub, err := nc.ChanSubscribe("wallets."+wData.Address+".transactions", txChan)
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
						Address: wData.Address,
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
					FeaturedAssetQty: humanize.Commaf(float64(qty) / math.Pow(10, float64(stelo.AssetScale))),
				})
				if err != nil {
					panic(err)
				}
				sse.MergeFragments(buff.String())
			case <-r.Context().Done():
				sub.Unsubscribe()
				break loop
			}
		}
	}
}

func WalletAssets(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		wData := sessions.GetWallet(r.Context())

		user, err := db.Q.GetUserById(r.Context(), uData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pfp := ""
		if user.DiscordPfp != nil {
			pfp = *user.DiscordPfp
		}

		accResult, err := db.Q.GetAccountBalancesByWalletAddr(r.Context(), wData.Address)
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
				WalletAddr:   wData.Address,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				ActivePage: "assets",
				WalletAddr: wData.Address,
			},
			PageData: templates.DataPageWalletAssets{
				WalletAddr: wData.Address,
				SteloSummary: templates.DataComponentSteloSummary{
					FeaturedAsset:    "Stelo",
					FeaturedAssetQty: humanize.Commaf(steloBal),
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
		wData := sessions.GetWallet(r.Context())

		sse := datastar.NewSSE(w, r)

		txChan := make(chan *nats.Msg)
		sub, err := nc.ChanSubscribe("wallets."+wData.Address+".transactions", txChan)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

	loop:
		for {
			select {
			case <-txChan:
				accResult, err := db.Q.GetAccountBalancesByWalletAddr(r.Context(), wData.Address)
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
					FeaturedAssetQty: humanize.Commaf(steloBal),
				})
				err = tmpls.ExecuteTemplate(buff, "components/assets", templates.DataComponentAssets{
					Assets: assets,
				})
				if err != nil {
					panic(err)
				}
				sse.MergeFragments(buff.String())
			case <-r.Context().Done():
				sub.Unsubscribe()
				break loop
			}
		}
	}
}

func WalletTransact(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		wData := sessions.GetWallet(r.Context())

		user, err := db.Q.GetUserById(r.Context(), uData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pfp := ""
		if user.DiscordPfp != nil {
			pfp = *user.DiscordPfp
		}

		accBals, err := db.Q.GetAccountBalancesByWalletAddr(r.Context(), wData.Address)
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
				WalletAddr:     wData.Address,
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
			} else if ds.Tx.Type == "withdraw" {
				withdrawRequests, err := db.Q.GetWithdrawRequests(r.Context(), wData.Id)
				if err != nil {
					w.WriteHeader(http.StatusInternalServerError)
					return
				}
				txIds := make([]int64, 0, len(withdrawRequests))
				for _, d := range withdrawRequests {
					txIds = append(txIds, d.ID)
				}

				assetsResult, err := db.Q.GetTransfersAssetsByTxIds(r.Context(), txIds)
				if err != nil {
					w.WriteHeader(http.StatusInternalServerError)
					return
				}

				withdrawReqs := make([]templates.DataWithdrawRequest, 0, len(withdrawRequests))
				for _, w := range withdrawRequests {
					assets := make([]templates.DataWithdrawRequestAsset, 0)
					for _, a := range assetsResult {
						if a.TransactionID == w.ID {
							assets = append(assets, templates.DataWithdrawRequestAsset{
								Name: a.Name,
								Qty:  float64(a.Amount) * math.Pow(10, float64(a.AssetScale)),
							})
						}
					}

					withdrawReqs = append(withdrawReqs, templates.DataWithdrawRequest{
						Withdrawer: w.DebitAddress,
						WithdrawId: w.ID,
						Assets:     assets,
					})
				}

				allAssetsResult, err := db.Q.GetLedgersByCode(r.Context(), int32(accounts.Item))
				if err != nil {
					w.WriteHeader(http.StatusInternalServerError)
					return
				}
				allAssets := make([]templates.DataAsset, 0, len(allAssetsResult))
				for _, a := range allAssetsResult {
					allAssets = append(allAssets, templates.DataAsset{
						LedgerId: a.ID,
						Name:     a.Name,
						// Qty:      0,
					})
				}

				data.WithdrawRequests = withdrawReqs
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
			sse.MergeFragments(buff.String())
			return
		} else {
			tmplData = templates.DataLayoutApp{
				Title:       "Home",
				Description: "Wallet homepage",
				NavData: templates.DataComponentAppNav{
					WalletAddr:   wData.Address,
					ProfileImage: pfp,
					Username:     user.DiscordUsername,
				},
				MenuData: templates.DataComponentAppMenu{
					ActivePage: "transact",
					WalletAddr: wData.Address,
				},
				PageData: templates.DataPageWalletTransact{
					WalletAddr: wData.Address,
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

func WalletApproveWithdraw(tmpls *templates.Tmpls, db *database.Database, nc *nats.Conn) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())

		// Verify TX belongs to wallet
		depoTxId, err := strconv.Atoi(chi.URLParam(r, "withdraw_tx_id"))
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		res, err := db.Q.GetTransaction(r.Context(), int64(depoTxId))
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		if res.CreditWalletID != wData.Id {
			w.WriteHeader(http.StatusForbidden)
			return
		}

		tx, err := db.Pool.Begin(r.Context())
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer tx.Rollback(r.Context())
		qtx := db.Q.WithTx(tx)

		// Finalize the TX
		accounts.FinalizeTransaction(r.Context(), qtx, accounts.FinalizeInput{
			TxId:   int64(depoTxId),
			Status: accounts.TxPostPending,
		})

		err = tx.Commit(r.Context())
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		// Render new page and serve
		withdrawRequests, err := db.Q.GetWithdrawRequests(r.Context(), wData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		txIds := make([]int64, 0, len(withdrawRequests))
		for _, d := range withdrawRequests {
			txIds = append(txIds, d.ID)
		}

		assetsResult, err := db.Q.GetTransfersAssetsByTxIds(r.Context(), txIds)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		withdrawReqs := make([]templates.DataWithdrawRequest, 0, len(withdrawRequests))
		for _, w := range withdrawRequests {
			assets := make([]templates.DataWithdrawRequestAsset, 0)
			for _, a := range assetsResult {
				if a.TransactionID == w.ID {
					assets = append(assets, templates.DataWithdrawRequestAsset{
						Name: a.Name,
						Qty:  float64(a.Amount) * math.Pow(10, float64(a.AssetScale)),
					})
				}
			}

			withdrawReqs = append(withdrawReqs, templates.DataWithdrawRequest{
				Withdrawer: w.DebitAddress,
				WithdrawId: w.ID,
				Assets:     assets,
			})
		}

		allAssetsResult, err := db.Q.GetLedgersByCode(r.Context(), int32(accounts.Item))
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		allAssets := make([]templates.DataAsset, 0, len(allAssetsResult))
		for _, a := range allAssetsResult {
			allAssets = append(allAssets, templates.DataAsset{
				LedgerId: a.ID,
				Name:     a.Name,
				// Qty:      0,
			})
		}

		sse := datastar.NewSSE(w, r)

		buff := new(bytes.Buffer)
		err = tmpls.ExecuteTemplate(buff, "components/withdraw-requests", templates.DataPageWalletTransact{
			WalletAddr:       wData.Address,
			WithdrawRequests: withdrawReqs,
		})
		if err != nil {
			panic(err)
		}

		sse.MergeFragments(buff.String())
	}
}

func WalletCreateTransaction(tmpls *templates.Tmpls, db *database.Database, nc *nats.Conn) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())

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
		idRows, err := db.Q.GetWalletIdsByAddr(r.Context(), []string{wData.Address, recipient})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		debitIndx := slices.IndexFunc(idRows, func(r gensql.GetWalletIdsByAddrRow) bool {
			return r.Address == recipient
		})
		creditIndx := slices.IndexFunc(idRows, func(r gensql.GetWalletIdsByAddrRow) bool {
			return r.Address == wData.Address
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
		qtx := db.Q.WithTx(tx)

		code := accounts.TxUserToUser
		creditId := idRows[creditIndx].ID
		debitId := idRows[debitIndx].ID
		if idRows[creditIndx].Code == int32(accounts.DAL) {
			code = accounts.TxSysUser
		}
		if txType == "deposit" {
			code = accounts.TxWarehouseTransfer
			creditId = debitId
			debitId = idRows[creditIndx].ID
		}
		_, err = accounts.CreateTransaction(r.Context(), qtx, nc, accounts.TxInput{
			DebitWalletId:  debitId,
			CreditWalletId: creditId,
			Code:           code,
			Memo:           memo,
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
		sData := sessions.GetUser(r.Context())
		wData := sessions.GetWallet(r.Context())

		user, err := db.Q.GetUserById(r.Context(), sData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pfp := ""
		if user.DiscordPfp != nil {
			pfp = *user.DiscordPfp
		}
		txs := make([]templates.DataTransaction, 0)

		txsRes, err := db.Q.GetTransactionsByWalletId(r.Context(), gensql.GetTransactionsByWalletIdParams{
			DebitWalletID: wData.Id,
			Limit:         50,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		for _, tx := range txsRes {
			direction := "outgoing"
			recip := tx.DebitAddress
			if tx.DebitWalletID == wData.Id {
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
				WalletAddr:   wData.Address,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				ActivePage: "history",
				WalletAddr: wData.Address,
			},
			PageData: templates.DataPageWalletTransactions{
				WalletAddr:   wData.Address,
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
		wData := sessions.GetWallet(r.Context())

		sse := datastar.NewSSE(w, r)

		txChan := make(chan *nats.Msg)
		sub, err := nc.ChanSubscribe("wallets."+wData.Address+".transactions", txChan)
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
					DebitWalletID: wData.Id,
					Limit:         50,
				})
				if err != nil {
					w.WriteHeader(http.StatusInternalServerError)
					return
				}

				for _, tx := range txsRes {
					direction := "outgoing"
					recip := tx.DebitAddress
					if tx.DebitWalletID == wData.Id {
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
						WalletAddr:     wData.Address,
						OnlyRenderPage: true,
						Transactions:   txs,
					},
				}

				buff := new(bytes.Buffer)
				err = tmpls.ExecuteTemplate(buff, "pages/wallet-transactions", tmplData)
				if err != nil {
					panic(err)
				}
				sse.MergeFragments(buff.String())
			case <-r.Context().Done():
				sub.Unsubscribe()
				break loop
			}
		}
	}
}

func Wallets(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())

		user, err := db.Q.GetUserById(r.Context(), uData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pfp := ""
		if user.DiscordPfp != nil {
			pfp = *user.DiscordPfp
		}

		// Get wallets and format them for the template
		wallets, err := db.Q.GetWalletsByUsrIdAndCodes(r.Context(), gensql.GetWalletsByUsrIdAndCodesParams{
			UserID:      uData.Id,
			WalletCodes: []int32{int32(accounts.PersonalAcc), int32(accounts.GeneralAcc)},
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		primaryAddr := ""
		walletsFmtd := make([]templates.DataPageWalletsWallet, 0, len(wallets))
		for _, w := range wallets {
			walletsFmtd = append(walletsFmtd, templates.DataPageWalletsWallet{
				Addr:       w.Address,
				IsPersonal: accounts.AccountCode(w.Code) == accounts.PersonalAcc,
				IsAdmin:    accounts.PermAdmin&accounts.Permission(w.Permissions) == accounts.PermAdmin,
			})

			if accounts.AccountCode(w.Code) == accounts.PersonalAcc {
				primaryAddr = w.Address
			}
		}
		if primaryAddr == "" {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		tmplData := templates.DataLayoutApp{
			Title:       "Wallets",
			Description: "A list of all your wallets, to edit and select them",
			NavData: templates.DataComponentAppNav{
				WalletAddr:   primaryAddr,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				ActivePage: "wallets",
				WalletAddr: primaryAddr,
			},
			PageData: templates.DataPageWallets{
				Wallets: walletsFmtd,
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/wallets", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func WalletSettings(tmpls *templates.Tmpls, db *database.Database, sessionsKV jetstream.KeyValue) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		wData := sessions.GetWallet(r.Context())

		user, err := db.Q.GetUserById(r.Context(), uData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pfp := ""
		if user.DiscordPfp != nil {
			pfp = *user.DiscordPfp
		}

		// Get Token count
		keyLstnr, err := sessionsKV.ListKeysFiltered(r.Context(), "wallets."+strconv.FormatInt(wData.Id, 10)+".sessions.*")
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer keyLstnr.Stop()
		tknQty := 0

		for range keyLstnr.Keys() {
			tknQty++
		}

		// Get wallets and format them for the template
		usrsOnWallet, err := db.Q.GetUsersOnWallet(r.Context(), wData.Address)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		usrs := make([]templates.DataPageWalletSettingsUser, 0)
		for _, usr := range usrsOnWallet {
			perms := make([]string, 0, 10)
			// TODO: make this not... exist LOL
			if accounts.PermAdmin&accounts.Permission(usr.Permissions) == accounts.PermAdmin {
				perms = append(perms, "admin")
			}
			if accounts.PermReadBals&accounts.Permission(usr.Permissions) == accounts.PermReadBals {
				perms = append(perms, "read")
			}

			usrs = append(usrs, templates.DataPageWalletSettingsUser{
				IsUser:      usr.UserID == uData.Id,
				Name:        usr.DiscordUsername,
				Permissions: perms,
			})
		}

		tmplData := templates.DataLayoutApp{
			Title:       "Wallet Settings",
			Description: "Settings for your wallet, to add and remove users and change their permissions.",
			NavData: templates.DataComponentAppNav{
				WalletAddr:   wData.Address,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				ActivePage: "wallet-settings",
				WalletAddr: wData.Address,
			},
			PageData: templates.DataPageWalletSettings{
				WalletAddr:     wData.Address,
				OnlyRenderPage: false,
				Users:          usrs,
				TokenQty:       tknQty,
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/wallet-settings", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func WalletAddUser(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		wData := sessions.GetWallet(r.Context())

		type bodyInput struct {
			Username string `json:"username"`
		}
		var body bodyInput
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		// Get userid
		userId, err := db.Q.GetUserIdByDiscordName(r.Context(), body.Username)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		// Add to wallet permissions
		_, err = db.Q.InsertWalletPermission(r.Context(), gensql.InsertWalletPermissionParams{
			WalletID:    wData.Id,
			UserID:      userId,
			Permissions: int64(accounts.PermReadBals),
			UpdatedAt:   time.Now(),
			CreatedAt:   time.Now(),
		})

		// Get wallets and format them for the template
		usrsOnWallet, err := db.Q.GetUsersOnWallet(r.Context(), wData.Address)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		usrs := make([]templates.DataPageWalletSettingsUser, 0)
		for _, usr := range usrsOnWallet {
			perms := make([]string, 0, 10)
			// TODO: make this not... exist LOL
			if accounts.PermAdmin&accounts.Permission(usr.Permissions) == accounts.PermAdmin {
				perms = append(perms, "admin")
			}
			if accounts.PermReadBals&accounts.Permission(usr.Permissions) == accounts.PermReadBals {
				perms = append(perms, "read")
			}

			usrs = append(usrs, templates.DataPageWalletSettingsUser{
				IsUser:      usr.UserID == uData.Id,
				Name:        usr.DiscordUsername,
				Permissions: perms,
			})
		}

		tmplData := templates.DataLayoutApp{
			PageData: templates.DataPageWalletSettings{
				WalletAddr:     wData.Address,
				OnlyRenderPage: true,
				Users:          usrs,
			},
		}

		sse := datastar.NewSSE(w, r)
		buff := new(bytes.Buffer)
		err = tmpls.ExecuteTemplate(buff, "pages/wallet-settings", tmplData)
		if err != nil {
			panic(err)
		}
		sse.MergeFragments(buff.String())
	}
}

func WalletCreateToken(tmpls *templates.Tmpls, db *database.Database, sessionsKV jetstream.KeyValue) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		wData := sessions.GetWallet(r.Context())

		// Create session
		sid := uniuri.NewLen(27)
		sData := sessions.WalletData{
			Id:      wData.Id,
			Address: wData.Address,
		}
		bitties, err := json.Marshal(sData)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		_, err = sessionsKV.Create(r.Context(), "wallets."+strconv.FormatInt(wData.Id, 10)+".sessions."+sid, bitties)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		// Render the template out now

		keyLstnr, err := sessionsKV.ListKeysFiltered(r.Context(), "wallets."+strconv.FormatInt(wData.Id, 10)+".sessions.*")
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer keyLstnr.Stop()
		tknQty := 0

		for range keyLstnr.Keys() {
			tknQty++
		}

		// Get wallets and format them for the template
		usrsOnWallet, err := db.Q.GetUsersOnWallet(r.Context(), wData.Address)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		usrs := make([]templates.DataPageWalletSettingsUser, 0)
		for _, usr := range usrsOnWallet {
			perms := make([]string, 0, 10)
			// TODO: make this not... exist LOL
			if accounts.PermAdmin&accounts.Permission(usr.Permissions) == accounts.PermAdmin {
				perms = append(perms, "admin")
			}
			if accounts.PermReadBals&accounts.Permission(usr.Permissions) == accounts.PermReadBals {
				perms = append(perms, "read")
			}

			usrs = append(usrs, templates.DataPageWalletSettingsUser{
				IsUser:      usr.UserID == uData.Id,
				Name:        usr.DiscordUsername,
				Permissions: perms,
			})
		}

		tmplData := templates.DataLayoutApp{
			PageData: templates.DataPageWalletSettings{
				WalletAddr:     wData.Address,
				OnlyRenderPage: true,
				Users:          usrs,
				Token:          "stlw_" + sid,
				TokenQty:       tknQty,
			},
		}

		sse := datastar.NewSSE(w, r)
		buff := new(bytes.Buffer)
		err = tmpls.ExecuteTemplate(buff, "pages/wallet-settings", tmplData)
		if err != nil {
			panic(err)
		}
		sse.MergeFragments(buff.String())
	}
}

func WalletDeleteTokens(tmpls *templates.Tmpls, db *database.Database, sessionsKV jetstream.KeyValue) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		wData := sessions.GetWallet(r.Context())

		// Delete all sessions
		keyLstnr, err := sessionsKV.ListKeysFiltered(r.Context(), "wallets."+strconv.FormatInt(wData.Id, 10)+".sessions.*")
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer keyLstnr.Stop()
		for key := range keyLstnr.Keys() {
			sessionsKV.Delete(r.Context(), key)
		}

		// Render the template out now
		// Get wallets and format them for the template
		usrsOnWallet, err := db.Q.GetUsersOnWallet(r.Context(), wData.Address)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		usrs := make([]templates.DataPageWalletSettingsUser, 0)
		for _, usr := range usrsOnWallet {
			perms := make([]string, 0, 10)
			// TODO: make this not... exist LOL
			if accounts.PermAdmin&accounts.Permission(usr.Permissions) == accounts.PermAdmin {
				perms = append(perms, "admin")
			}
			if accounts.PermReadBals&accounts.Permission(usr.Permissions) == accounts.PermReadBals {
				perms = append(perms, "read")
			}

			usrs = append(usrs, templates.DataPageWalletSettingsUser{
				IsUser:      usr.UserID == uData.Id,
				Name:        usr.DiscordUsername,
				Permissions: perms,
			})
		}

		tmplData := templates.DataLayoutApp{
			PageData: templates.DataPageWalletSettings{
				WalletAddr:     wData.Address,
				OnlyRenderPage: true,
				Users:          usrs,
				TokenQty:       0,
			},
		}

		sse := datastar.NewSSE(w, r)
		buff := new(bytes.Buffer)
		err = tmpls.ExecuteTemplate(buff, "pages/wallet-settings", tmplData)
		if err != nil {
			panic(err)
		}
		sse.MergeFragments(buff.String())
	}
}

func WalletRemoveUser(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())
		uData := sessions.GetUser(r.Context())
		discordUsername := chi.URLParam(r, "discord_username")

		// TODO: Prevent them from removing themselves lol (big bozo energy if they do though)

		// Remove user from wallet
		err := db.Q.DeleteWalletPerm(r.Context(), gensql.DeleteWalletPermParams{
			WalletID:        wData.Id,
			DiscordUsername: discordUsername,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		// Get wallets and format them for the template
		usrsOnWallet, err := db.Q.GetUsersOnWallet(r.Context(), wData.Address)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		usrs := make([]templates.DataPageWalletSettingsUser, 0)
		for _, usr := range usrsOnWallet {
			perms := make([]string, 0, 10)
			// TODO: make this not... exist LOL
			if accounts.PermAdmin&accounts.Permission(usr.Permissions) == accounts.PermAdmin {
				perms = append(perms, "admin")
			}
			if accounts.PermReadBals&accounts.Permission(usr.Permissions) == accounts.PermReadBals {
				perms = append(perms, "read")
			}

			usrs = append(usrs, templates.DataPageWalletSettingsUser{
				IsUser:      usr.UserID == uData.Id,
				Name:        usr.DiscordUsername,
				Permissions: perms,
			})
		}

		tmplData := templates.DataLayoutApp{
			PageData: templates.DataPageWalletSettings{
				WalletAddr:     wData.Address,
				OnlyRenderPage: true,
				Users:          usrs,
			},
		}

		sse := datastar.NewSSE(w, r)
		buff := new(bytes.Buffer)
		err = tmpls.ExecuteTemplate(buff, "pages/wallet-settings", tmplData)
		if err != nil {
			panic(err)
		}
		sse.MergeFragments(buff.String())
	}
}

func WalletsCreate(db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())

		tx, err := db.Pool.Begin(r.Context())
		defer tx.Rollback(r.Context())
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		qtx := db.Q.WithTx(tx)
		_, addr, err := accounts.CreateGeneralWallet(r.Context(), qtx, uData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		err = tx.Commit(r.Context())
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		// Redirect to the new wallet homepage
		sse := datastar.NewSSE(w, r)
		sse.Redirect("/app/wallets/" + addr)
	}
}

func WalletUserSettings(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		wData := sessions.GetWallet(r.Context())
		username := chi.URLParam(r, "discord_username")

		user, err := db.Q.GetUserById(r.Context(), uData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pfp := ""
		if user.DiscordPfp != nil {
			pfp = *user.DiscordPfp
		}

		// Get wallets and format them for the template
		usrPerms, err := db.Q.GetUserOnWallet(r.Context(), gensql.GetUserOnWalletParams{
			WalletID:        wData.Id,
			DiscordUsername: username,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		perms := []string{"admin", "read"}
		enabledPerms := make([]string, 0)

		// TODO: make this not... exist LOL
		if accounts.PermAdmin&accounts.Permission(usrPerms) == accounts.PermAdmin {
			enabledPerms = append(enabledPerms, "admin")
		}
		if accounts.PermReadBals&accounts.Permission(usrPerms) == accounts.PermReadBals {
			enabledPerms = append(enabledPerms, "read")
		}

		tmplData := templates.DataLayoutApp{
			Title:       "Wallet User Settings",
			Description: "Settings for a user your wallet, where you can change their permissions.",
			NavData: templates.DataComponentAppNav{
				WalletAddr:   wData.Address,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				ActivePage: "wallet-user-settings",
				WalletAddr: wData.Address,
			},
			PageData: templates.DataPageWalletUserSettings{
				WalletAddr:   wData.Address,
				Username:     username,
				Perms:        perms,
				EnabledPerms: enabledPerms,
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/wallet-user-settings", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func UpdateWalletUserSettings(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())
		username := chi.URLParam(r, "discord_username")

		type bodyInput struct {
			Perms map[string]bool `json:"perms"`
		}
		var body bodyInput
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		// convert body perms to number
		perms := []string{"admin", "read"}
		permsEnabled := make([]string, 0)
		permsNum := accounts.PermNone
		// TODO: make this not... exist LOL
		if body.Perms["admin"] {
			permsNum += accounts.PermAdmin
			permsEnabled = append(permsEnabled, "admin")
		}
		if body.Perms["read"] {
			permsNum += accounts.PermReadBals
			permsEnabled = append(permsEnabled, "read")
		}

		// Update their perms
		err := db.Q.UpdateWalletPerm(r.Context(), gensql.UpdateWalletPermParams{
			Permissions:     int64(permsNum),
			WalletID:        wData.Id,
			DiscordUsername: username,
		})

		tmplData := templates.DataLayoutApp{
			PageData: templates.DataPageWalletUserSettings{
				OnlyRenderPage: true,
				WalletAddr:     wData.Address,
				Username:       username,
				Perms:          perms,
				EnabledPerms:   permsEnabled,
			},
		}

		sse := datastar.NewSSE(w, r)

		buff := new(bytes.Buffer)
		err = tmpls.ExecuteTemplate(buff, "pages/wallet-user-settings", tmplData)
		if err != nil {
			panic(err)
		}
		sse.MergeFragments(buff.String())
	}
}

func WalletMarket(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		wData := sessions.GetWallet(r.Context())

		user, err := db.Q.GetUserById(r.Context(), uData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pfp := ""
		if user.DiscordPfp != nil {
			pfp = *user.DiscordPfp
		}

		coinSwap := templates.DataComponentCoinSwap{
			WalletAddr:     wData.Address,
			ActiveCoin:     "hexcoin",
			ExpectedReturn: "0",
			Rate:           "",
			Qty:            0,
		}
		if r.URL.Query().Has("datastar") {
			type input struct {
				Qty        int    `json:"qty"`
				ActiveCoin string `json:"activeCoin"`
			}
			var ds input
			err = json.Unmarshal([]byte(r.URL.Query().Get("datastar")), &ds)
			if err != nil {
				w.WriteHeader(http.StatusBadRequest)
				return
			}
			coinSwap.Qty = ds.Qty
			coinSwap.ActiveCoin = ds.ActiveCoin
		}
		bals, err := db.Q.GetAccountBalancesByWalletAddr(r.Context(), "COINSWAP")
		if err != nil && !errors.Is(err, pgx.ErrNoRows) {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		if !errors.Is(err, pgx.ErrNoRows) && len(bals) >= 2 {
			foundNum := 0
			stelo := bals[0]
			hexcoin := bals[0]
			for _, b := range bals {
				if b.AssetName == "stelo" {
					stelo = b
					foundNum++
					continue
				}
				if b.AssetName == "hexcoin" {
					hexcoin = b
					foundNum++
					continue
				}
			}
			if foundNum != 2 {
				goto skipSwap
			}
			stlFmtd := float64(stelo.DebitBalance) / math.Pow(10, float64(stelo.AssetScale))
			coinSwap.Rate = fmt.Sprintf("%v stl / 1 hex", humanize.FormatFloat("", stlFmtd/float64(hexcoin.DebitBalance)))

			if coinSwap.Qty != 0 {
				volNum := float64(stelo.DebitBalance) * float64(hexcoin.DebitBalance)
				if coinSwap.ActiveCoin == "hexcoin" {
					stlRemaining := volNum / (float64(hexcoin.DebitBalance) + float64(coinSwap.Qty))
					stlToPay := int64(float64(stelo.DebitBalance) - stlRemaining)
					coinSwap.ExpectedReturn = humanize.FormatFloat("", float64(stlToPay)/math.Pow(10, float64(stelo.AssetScale)))
				} else {
					qtyScaled := math.Pow(10, float64(stelo.AssetScale)) * float64(coinSwap.Qty)
					hexRemaining := math.Ceil(volNum / (float64(stelo.DebitBalance) + qtyScaled))
					hexToPay := int64(hexcoin.DebitBalance - int64(hexRemaining))
					coinSwap.ExpectedReturn = humanize.Comma(hexToPay)
				}
			}
		}
	skipSwap:

		if r.URL.Query().Has("datastar") {
			sse := datastar.NewSSE(w, r)
			buff := new(bytes.Buffer)
			err = tmpls.ExecuteTemplate(buff, "pages/wallet-market", templates.DataLayoutApp{
				PageData: templates.DataPageWalletMarket{
					OnlyRenderPage: true,
					CoinSwap:       coinSwap,
				},
			})
			if err != nil {
				panic(err)
			}
			sse.MergeFragments(buff.String())
			return
		}

		tmplData := templates.DataLayoutApp{
			Title:       "Market",
			Description: "Marketplace for buying, selling, and trading items",
			NavData: templates.DataComponentAppNav{
				WalletAddr:   wData.Address,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				ActivePage: "market",
				WalletAddr: wData.Address,
			},
			PageData: templates.DataPageWalletMarket{
				// OnlyRenderPage: false,
				CoinSwap: coinSwap,
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/wallet-market", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func ExecuteCoinSwap(tmpls *templates.Tmpls, db *database.Database, nc *nats.Conn) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())

		type input struct {
			Qty        int    `json:"qty"`
			ActiveCoin string `json:"activeCoin"`
		}
		var body input
		err := json.NewDecoder(r.Body).Decode(&body)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		bals, err := db.Q.GetAccountBalancesByWalletAddr(r.Context(), "COINSWAP")
		if err != nil && !errors.Is(err, pgx.ErrNoRows) {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		foundNum := 0
		stelo := bals[0]
		hexcoin := bals[0]
		for _, b := range bals {
			if b.AssetName == "stelo" {
				stelo = b
				foundNum++
				continue
			}
			if b.AssetName == "hexcoin" {
				hexcoin = b
				foundNum++
				continue
			}
		}
		if foundNum != 2 {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		volNum := float64(stelo.DebitBalance) * float64(hexcoin.DebitBalance)

		tx, err := db.Pool.Begin(r.Context())
		defer tx.Rollback(r.Context())
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		qtx := db.Q.WithTx(tx)

		memo := "coin swap"
		if body.ActiveCoin == "hexcoin" {
			stlRemaining := volNum / (float64(hexcoin.DebitBalance) + float64(body.Qty))
			stlToPay := int64(float64(stelo.DebitBalance) - stlRemaining)

			// send hex
			_, err = accounts.CreateTransaction(r.Context(), qtx, nc, accounts.TxInput{
				DebitWalletId:  stelo.WalletID,
				CreditWalletId: wData.Id,
				Code:           accounts.TxUserToUser,
				Memo:           &memo,
				Assets: []accounts.TxAssets{{
					LedgerId: hexcoin.LedgerID,
					Amount:   int64(body.Qty),
				}},
			})
			if err != nil {
				w.WriteHeader(http.StatusInternalServerError)
				return
			}
			// receive stelo
			_, err = accounts.CreateTransaction(r.Context(), qtx, nc, accounts.TxInput{
				DebitWalletId:  wData.Id,
				CreditWalletId: stelo.WalletID,
				Code:           accounts.TxUserToUser,
				Memo:           &memo,
				Assets: []accounts.TxAssets{{
					LedgerId: stelo.LedgerID,
					Amount:   stlToPay,
				}},
			})
			if err != nil {
				w.WriteHeader(http.StatusInternalServerError)
				return
			}
		} else {
			qtyScaled := math.Pow(10, float64(stelo.AssetScale)) * float64(body.Qty)
			hexRemaining := volNum / (float64(stelo.DebitBalance) + qtyScaled)
			hexToPay := int64(float64(hexcoin.DebitBalance) - hexRemaining)

			// send stelo
			_, err = accounts.CreateTransaction(r.Context(), qtx, nc, accounts.TxInput{
				DebitWalletId:  stelo.WalletID,
				CreditWalletId: wData.Id,
				Code:           accounts.TxUserToUser,
				Memo:           &memo,
				Assets: []accounts.TxAssets{{
					LedgerId: stelo.LedgerID,
					Amount:   int64(qtyScaled),
				}},
			})
			if err != nil {
				w.WriteHeader(http.StatusInternalServerError)
				return
			}
			// receive hex
			_, err = accounts.CreateTransaction(r.Context(), qtx, nc, accounts.TxInput{
				DebitWalletId:  wData.Id,
				CreditWalletId: stelo.WalletID,
				Code:           accounts.TxUserToUser,
				Memo:           &memo,
				Assets: []accounts.TxAssets{{
					LedgerId: hexcoin.LedgerID,
					Amount:   hexToPay,
				}},
			})
			if err != nil {
				w.WriteHeader(http.StatusInternalServerError)
				return
			}
		}

		tx.Commit(r.Context())

		// Update btn
		sse := datastar.NewSSE(w, r)
		sse.MergeFragmentf(`<button id="swap-btn" disabled class="border text-white mt-4 text-lg w-full">SWAPPED</button>`)
	}
}
