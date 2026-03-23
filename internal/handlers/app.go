package handlers

import (
	"bytes"
	"context"
	"fmt"
	"math"
	"net/http"
	"strconv"

	"github.com/dustin/go-humanize"
	"github.com/starfederation/datastar-go/datastar"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/internal/accounts"
	"github.com/stelofinance/stelofinance/internal/sessions"
	"github.com/stelofinance/stelofinance/web/templates"
)

func AppHome(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())

		tmplData := templates.LayoutApp{
			Title:       "Home",
			Description: "App homepage",
			NavData: templates.ComponentAppNav{
				Username: uData.BitCraftUsername,
			},
			MenuData: templates.ComponentAppMenu{
				ActivePage: "home",
			},
			PageData: templates.PageAppHome{
				Username: uData.BitCraftUsername,
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err := tmpls.ExecuteTemplate(w, "pages/app-home", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func loadAppAccountsPageData(ctx context.Context, db *database.Database, uData *sessions.UserData, onlyRenderPage bool) (templates.LayoutApp, error) {
	// Fetch data and render page
	ldgrsResult, err := db.Q.GetAllLedgers(ctx)
	if err != nil {
		return templates.LayoutApp{}, err
	}
	accsResult, err := db.Q.GetAccountsUserHasPerms(ctx, uData.Id)
	if err != nil {
		return templates.LayoutApp{}, err
	}

	var accs []templates.PageAppAccountsAccount
	for _, acc := range accsResult {
		isPrimary := false
		if acc.PrimaryUserID != nil && *acc.PrimaryUserID == uData.Id {
			isPrimary = true
		}
		bal := acc.DebitsPosted - acc.CreditsPosted - acc.CreditsPending
		if accounts.AccountCode(acc.AccountCode).IsCredit() {
			bal = acc.CreditsPosted - acc.DebitsPosted - acc.DebitsPending
		}
		accs = append(accs, templates.PageAppAccountsAccount{
			AccId:      acc.ID,
			Addr:       acc.Address,
			AccCode:    accounts.AccountCode(acc.AccountCode),
			IsPrimary:  isPrimary,
			LedgerCode: accounts.LedgerCode(acc.LedgerCode),
			LedgerName: acc.LedgerName,
			DisplayQty: humanize.Commaf(float64(bal) / math.Pow(10, float64(acc.AssetScale))),
		})
	}
	var ldgrs []templates.PageAppAccountsLedger
	for _, ldgr := range ldgrsResult {
		ldgrs = append(ldgrs, templates.PageAppAccountsLedger{
			ID:   ldgr.ID,
			Name: ldgr.Name,
		})
	}

	return templates.LayoutApp{
		Title:       "Home",
		Description: "App homepage",
		NavData: templates.ComponentAppNav{
			Username: uData.BitCraftUsername,
		},
		MenuData: templates.ComponentAppMenu{
			ActivePage: "accounts",
		},
		PageData: templates.PageAppAccounts{
			OnlyRenderPage: onlyRenderPage,
			Ledgers:        ldgrs,
			Accounts:       accs,
		},
	}, nil
}

func AppAccounts(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())

		tmplData, err := loadAppAccountsPageData(r.Context(), db, uData, false)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/app-accounts", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func AppCreateAccount(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		fmt.Println("posting")
		uData := sessions.GetUser(r.Context())

		ldgrId, err := strconv.Atoi(r.FormValue("ledger"))
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		qtx, err := db.QTx(r.Context(), database.WithForeignKeys)
		defer qtx.Cleanup()
		_, err = accounts.CreateAccount(r.Context(), qtx.Q(), accounts.CreateAccountInput{
			OwnerId: uData.Id,
			// Address:  "",
			Webhook:  nil,
			LedgerId: int64(ldgrId),
			Code:     accounts.GA,
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		qtx.Commit()

		tmplData, err := loadAppAccountsPageData(r.Context(), db, uData, true)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		sse := datastar.NewSSE(w, r)

		buff := new(bytes.Buffer)
		err = tmpls.ExecuteTemplate(buff, "pages/app-accounts", tmplData)
		if err != nil {
			panic(err)
		}
		sse.PatchElements(buff.String())
	}
}
