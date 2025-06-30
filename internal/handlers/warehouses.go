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
			PageData: templates.DataPageWarehouseDepositWithdraw{
				WalletAddr:      wData.Address,
				DepositRequests: dpstReqs,
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/warehouse-deposit-withdraw", tmplData)
		if err != nil {
			panic(err)
		}
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

		// Finalize the TX
		accounts.FinalizeTransaction(r.Context(), gensql.New(tx), accounts.FinalizeInput{
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

		sse.MergeFragments(string(buff.Bytes()))
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

		_, err = accounts.CreateWarehouseWallet(r.Context(), gensql.New(tx), accounts.CreateWarehouseInput{
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

		sse.MergeFragments(string(buff.Bytes()))
	}
}
