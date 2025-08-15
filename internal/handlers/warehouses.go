package handlers

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"net/http"
	"strconv"

	"github.com/cridenour/go-postgis"
	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/nats-io/nats.go"
	datastar "github.com/starfederation/datastar/sdk/go"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/database/gensql"
	"github.com/stelofinance/stelofinance/internal/accounts"
	"github.com/stelofinance/stelofinance/internal/sessions"
	"github.com/stelofinance/stelofinance/web/templates"
)

func Warehouses(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		// wData := sessions.GetWallet(r.Context())

		user, err := db.Q.GetUserById(r.Context(), uData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pfp := ""
		if user.DiscordPfp != nil {
			pfp = *user.DiscordPfp
		}

		result, err := db.Q.GetWalletsByUsrIdAndCodes(r.Context(), gensql.GetWalletsByUsrIdAndCodesParams{
			UserID:      uData.Id,
			WalletCodes: []int32{int32(accounts.WarehouseAcc)},
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		warehouses := make([]templates.DataWarehouse, 0)
		for _, wh := range result {
			loc := ""
			if wh.Location != nil {
				loc = fmt.Sprintf("N: %v, E: %v", wh.Location.X, wh.Location.Y)
			}
			warehouses = append(warehouses, templates.DataWarehouse{
				Addr:     wh.Address,
				Location: loc,
			})
		}

		tmplData := templates.DataLayoutApp{
			Title:       "Warehouses",
			Description: "All of your warehouses",
			NavData: templates.DataComponentAppNav{
				ForWarehouse: true,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				ForWarehouse: true,
				ActivePage:   "warehouses",
			},
			PageData: templates.DataPageWarehouses{
				Warehouses: warehouses,
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/warehouses", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func WarehouseHome(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
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

		bals, err := db.Q.GetAccountBalancesByWalletIdAndCode(r.Context(), gensql.GetAccountBalancesByWalletIdAndCodeParams{
			Codes:    []int32{int32(accounts.WarehouseCollatAcc), int32(accounts.WarehouseCollatLkdAcc)},
			WalletID: wData.Id,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		var collatAcc gensql.GetAccountBalancesByWalletIdAndCodeRow
		var collatAccLkd gensql.GetAccountBalancesByWalletIdAndCodeRow
		for _, b := range bals {
			if b.Code == int32(accounts.WarehouseCollatAcc) {
				collatAcc = b
			} else if b.Code == int32(accounts.WarehouseCollatLkdAcc) {
				collatAccLkd = b
			}
		}

		free := float64(collatAcc.DebitBalance) / math.Pow(10, float64(collatAcc.AssetScale))
		total := float64(collatAcc.DebitBalance+collatAccLkd.DebitBalance) / math.Pow(10, float64(collatAcc.AssetScale))

		tmplData := templates.DataLayoutApp{
			Title:       "Home",
			Description: "Homepage with summaries of information for your warehouse",
			NavData: templates.DataComponentAppNav{
				WalletAddr:   wData.Address,
				ForWarehouse: true,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				WalletAddr:   wData.Address,
				ForWarehouse: true,
				ActivePage:   "home",
			},
			PageData: templates.DataPageWarehouseHome{
				RemainingPercent: (free / total) * 100,
				FreeCollat:       free,
				TotalCollat:      total,
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/warehouse-home", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func WarehouseDepositWithdraw(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
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

		depositsResult, err := db.Q.GetDepositRequests(r.Context(), wData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		txIds := make([]int64, 0, len(depositsResult))
		for _, d := range depositsResult {
			txIds = append(txIds, d.ID)
		}

		assetsResult, err := db.Q.GetTransfersAssetsByTxIds(r.Context(), txIds)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		dpstReqs := make([]templates.DataDepositRequest, 0, len(depositsResult))
		for _, d := range depositsResult {
			assets := make([]templates.DataDepositRequestAsset, 0)
			for _, a := range assetsResult {
				if a.TransactionID == d.ID {
					assets = append(assets, templates.DataDepositRequestAsset{
						Name: a.Name,
						Qty:  float64(a.Amount) * math.Pow(10, float64(a.AssetScale)),
					})
				}
			}

			dpstReqs = append(dpstReqs, templates.DataDepositRequest{
				Depositor: d.DebitUsername,
				DepositId: d.ID,
				Assets:    assets,
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

		data := templates.DataPageWarehouseDepositWithdraw{
			WalletAddr:      wData.Address,
			DepositRequests: dpstReqs,

			Assets: allAssets,
		}

		if r.URL.Query().Has("datastar") {
			data.OnlyRenderPage = true
			type input struct {
				Search            string `json:"withdrawSearch"`
				WithdrawRecipient string `json:"withdrawRecipient"`
				Assets            map[string]struct {
					Id  string `json:"id"`
					Qty int    `json:"qty"`
				} `json:"assets"`
			}
			var ds input
			err = json.Unmarshal([]byte(r.URL.Query().Get("datastar")), &ds)
			if err != nil {
				w.WriteHeader(http.StatusBadRequest)
				return
			}

			data.WithdrawRecipient = ds.WithdrawRecipient

			if ds.WithdrawRecipient == "" && ds.Search != "" {
				usrWallets, err := db.Q.SearchWalletAddrByDiscord(r.Context(), gensql.SearchWalletAddrByDiscordParams{
					DiscordUsername: "%" + ds.Search + "%",
					Limit:           5,
				})
				if err != nil {
					w.WriteHeader(http.StatusInternalServerError)
					return
				}
				suggestions := make([]templates.DataRecipientSuggestion, 0, len(usrWallets))
				for _, w := range usrWallets {
					suggestions = append(suggestions, templates.DataRecipientSuggestion{
						Value:      w.DiscordUsername,
						WalletAddr: w.Address,
					})
				}
				data.RecipientSuggestions = suggestions
			}

			for _, v := range ds.Assets {
				name := "N/A"
				vId, err := strconv.Atoi(v.Id)
				if err != nil {
					continue
				}
				for _, a := range allAssets {
					if a.LedgerId == int64(vId) {
						name = a.Name
						break
					}
				}

				data.AssetsSelected = append(data.AssetsSelected, templates.DataAsset{
					LedgerId: int64(vId),
					Name:     name,
					Qty:      v.Qty,
				})
			}

			sse := datastar.NewSSE(w, r)

			tmplData := templates.DataLayoutApp{
				PageData: data,
			}
			buff := new(bytes.Buffer)
			err = tmpls.ExecuteTemplate(buff, "pages/warehouse-deposit-withdraw", tmplData)
			if err != nil {
				w.WriteHeader(http.StatusBadRequest)
				return
			}
			sse.MergeFragments(buff.String())
			return
		}

		tmplData := templates.DataLayoutApp{
			Title:       "Deposit/Withdraw",
			Description: "Manage your warehouses deposit and withdraw requests.",
			NavData: templates.DataComponentAppNav{
				WalletAddr:   wData.Address,
				ForWarehouse: true,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				WalletAddr:   wData.Address,
				ForWarehouse: true,
				ActivePage:   "deposit-withdraw",
			},
			PageData: data,
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/warehouse-deposit-withdraw", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func CreateWithdraw(tmpls *templates.Tmpls, db *database.Database, nc *nats.Conn) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		// uData := sessions.GetUser(r.Context())
		wData := sessions.GetWallet(r.Context())

		type input struct {
			Search            string `json:"withdrawSearch"`
			WithdrawRecipient string `json:"withdrawRecipient"`
			Assets            map[string]struct {
				Id  string `json:"id"`
				Qty int    `json:"qty"`
			} `json:"assets"`
		}
		var body input
		err := json.NewDecoder(r.Body).Decode(&body)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		assets := make([]accounts.TxAssets, 0, len(body.Assets))
		for _, a := range body.Assets {
			aId, err := strconv.Atoi(a.Id)
			if err != nil {
				continue
			}

			assets = append(assets, accounts.TxAssets{
				LedgerId: int64(aId),
				Amount:   int64(a.Qty),
			})
		}

		credId, err := db.Q.GetWalletIdByAddr(r.Context(), body.WithdrawRecipient)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		tx, err := db.Pool.Begin(r.Context())
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer tx.Rollback(r.Context())
		qtx := db.Q.WithTx(tx)
		_, err = accounts.CreateTransaction(r.Context(), qtx, nc, accounts.TxInput{
			DebitWalletId:  wData.Id,
			CreditWalletId: credId,
			Code:           accounts.TxWarehouseTransfer,
			Memo:           nil,
			Assets:         assets,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		tx.Commit(r.Context())

		sse := datastar.NewSSE(w, r)
		sse.MergeFragments(`<button id="submit-btn" type="submit" disabled class="border border-neutral-500 text-xl w-full mt-4 py-2">CREATED!</button>`)
	}
}

func ApproveDeposit(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		wData := sessions.GetWallet(r.Context())

		// Verify TX belongs to wallet
		depoTxId, err := strconv.Atoi(chi.URLParam(r, "deposit_tx_id"))
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
		depositsResult, err := db.Q.GetDepositRequests(r.Context(), wData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		txIds := make([]int64, 0, len(depositsResult))
		for _, d := range depositsResult {
			txIds = append(txIds, d.ID)
		}

		assetsResult, err := db.Q.GetTransfersAssetsByTxIds(r.Context(), txIds)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		dpstReqs := make([]templates.DataDepositRequest, 0, len(depositsResult))
		for _, d := range depositsResult {
			assets := make([]templates.DataDepositRequestAsset, 0)
			for _, a := range assetsResult {
				if a.TransactionID == d.ID {
					assets = append(assets, templates.DataDepositRequestAsset{
						Name: a.Name,
						Qty:  float64(a.Amount) * math.Pow(10, float64(a.AssetScale)),
					})
				}
			}

			dpstReqs = append(dpstReqs, templates.DataDepositRequest{
				Depositor: d.DebitUsername,
				DepositId: d.ID,
				Assets:    assets,
			})
		}

		tmplData := templates.DataLayoutApp{
			PageData: templates.DataPageWarehouseDepositWithdraw{
				OnlyRenderPage:  true,
				WalletAddr:      wData.Address,
				DepositRequests: dpstReqs,
			},
		}

		sse := datastar.NewSSE(w, r)

		buff := new(bytes.Buffer)
		err = tmpls.ExecuteTemplate(buff, "pages/warehouse-deposit-withdraw", tmplData)
		if err != nil {
			panic(err)
		}

		sse.MergeFragments(buff.String())
	}
}

func CreateWarehouse(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())

		type Input struct {
			Form struct {
				Addr   string `json:"addr"`
				NCoord int    `json:"ncoord"`
				ECoord int    `json:"ecoord"`
			} `json:"form"`
		}
		var body Input
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		tx, err := db.Pool.Begin(r.Context())
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer tx.Rollback(r.Context())
		qtx := db.Q.WithTx(tx)

		_, err = accounts.CreateWarehouseWallet(r.Context(), qtx, accounts.CreateWarehouseInput{
			Addr:   body.Form.Addr,
			UserId: uData.Id,
			Location: postgis.Point{
				X: float64(body.Form.NCoord),
				Y: float64(body.Form.ECoord),
			},
			CollateralPercentage: 2000,
		})
		if err != nil {
			var pgErr *pgconn.PgError
			if errors.As(err, &pgErr) && pgErr.Code == "23505" {
				sse := datastar.NewSSE(w, r)
				sse.MergeFragments(`<p id="error-msg" class="text-red-500 text-sm">Address taken</p>`)
				return
			}
			// TODO: Check if it's an address collisison
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		err = tx.Commit(r.Context())
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		// Now just serve back the warehouses page
		result, err := db.Q.GetWalletsByUsrIdAndCodes(r.Context(), gensql.GetWalletsByUsrIdAndCodesParams{
			UserID:      uData.Id,
			WalletCodes: []int32{int32(accounts.WarehouseAcc)},
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		warehouses := make([]templates.DataWarehouse, 0)
		for _, wh := range result {
			loc := ""
			if wh.Location != nil {
				loc = fmt.Sprintf("N: %v, E: %v", wh.Location.X, wh.Location.Y)
			}
			warehouses = append(warehouses, templates.DataWarehouse{
				Addr:     wh.Address,
				Location: loc,
			})
		}

		tmplData := templates.DataLayoutApp{
			PageData: templates.DataPageWarehouses{
				OnlyRenderPage: true,
				Warehouses:     warehouses,
			},
		}

		sse := datastar.NewSSE(w, r)

		buff := new(bytes.Buffer)
		err = tmpls.ExecuteTemplate(buff, "pages/warehouses", tmplData)
		if err != nil {
			panic(err)
		}

		sse.MergeFragments(buff.String())
	}
}
