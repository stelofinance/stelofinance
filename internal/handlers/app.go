package handlers

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"math"
	"net/http"
	"strconv"

	"github.com/dustin/go-humanize"
	"github.com/go-chi/chi/v5"
	"github.com/starfederation/datastar-go/datastar"
	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/database/gensql"
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

func loadAppAccountPageData(ctx context.Context, db *database.Database, uData *sessions.UserData, accId int64, onlyPage bool) (templates.LayoutApp, error) {
	acc, err := db.Q.GetAccountAndLedgerById(ctx, accId)
	if err != nil {
		return templates.LayoutApp{}, err
	}

	permsResults, err := db.Q.GetUsersOnAccount(ctx, int64(accId))
	if err != nil {
		return templates.LayoutApp{}, err
	}

	var userPerms accounts.Permission
	users := make([]templates.PageAppAccountUser, 0, len(permsResults))
	for _, perm := range permsResults {
		users = append(users, templates.PageAppAccountUser{
			UserId:   perm.UserID,
			APId:     perm.ID,
			Username: perm.BitcraftUsername,
		})
		if perm.UserID == uData.Id {
			userPerms = accounts.Permission(perm.Permissions)
		}
	}

	isPrimary := false
	if acc.UserID != nil && *acc.UserID == uData.Id {
		isPrimary = true
	}

	return templates.LayoutApp{
		Title:       fmt.Sprintf("#%s / %s", acc.Address, acc.LedgerName),
		Description: "Account configuration",
		NavData: templates.ComponentAppNav{
			Username: uData.BitCraftUsername,
		},
		MenuData: templates.ComponentAppMenu{
			ActivePage: "account",
		},
		PageData: templates.PageAppAccount{
			OnlyRenderPage: onlyPage,
			AccountId:      acc.ID,
			Address:        acc.Address,
			LedgerName:     acc.LedgerName,
			IsAdmin:        userPerms.HasPerms(accounts.PermAdmin),
			IsPrimary:      isPrimary,
			UserId:         uData.Id,
			Users:          users,
		},
	}, nil
}

func AppAccount(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		accIdStr := chi.URLParam(r, "account_id")
		accId, err := strconv.Atoi(accIdStr)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		tmplData, err := loadAppAccountPageData(
			r.Context(),
			db,
			uData,
			int64(accId),
			false,
		)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/app-account", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func PutAccountUser(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		accIdStr := chi.URLParam(r, "account_id")
		accId, err := strconv.Atoi(accIdStr)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		type Body struct {
			Primary bool `json:"primary"`
		}
		var body Body
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		// Update primary user
		qtx, err := db.QTx(r.Context(), database.WithForeignKeys)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer qtx.Cleanup()

		var userId *int64
		// If primary is true, set to current user, otherwise keep nil
		// TODO: This does mean other admins on the same account could
		// be clearing the other admin from the primary user. Fix?
		if body.Primary {
			userId = &uData.Id
		}

		err = qtx.Q().UpdateAccountUserId(r.Context(), gensql.UpdateAccountUserIdParams{
			UserID: userId,
			ID:     int64(accId),
		})
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		qtx.Commit()

		// Update page
		tmplData, err := loadAppAccountPageData(
			r.Context(),
			db,
			uData,
			int64(accId),
			true,
		)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		sse := datastar.NewSSE(w, r)

		buff := new(bytes.Buffer)
		err = tmpls.ExecuteTemplate(buff, "pages/app-account", tmplData)
		if err != nil {
			panic(err)
		}
		sse.PatchElements(buff.String())
	}
}

func PostAccountUsers(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		accIdStr := chi.URLParam(r, "account_id")
		accId, err := strconv.Atoi(accIdStr)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		type Body struct {
			Username string `json:"addUsername"`
		}
		var body Body
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		// Update primary user
		qtx, err := db.QTx(r.Context(), database.WithForeignKeys)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer qtx.Cleanup()

		// TODO: query the userid from the username, then add perms
		// and merge in new html

		// qtx.Q().InsertAccountPerm(r.Context(), gensql.InsertAccountPermParams{
		// 	AccountID:   int64(accId),
		// 	UserID:      u,
		// 	Permissions: int64(accounts.PermAdmin),
		// 	UpdatedAt:   ,
		// 	CreatedAt:   time.Time{},
		// })

		qtx.Commit()

		// Update page
		tmplData, err := loadAppAccountPageData(
			r.Context(),
			db,
			uData,
			int64(accId),
			true,
		)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		sse := datastar.NewSSE(w, r)

		buff := new(bytes.Buffer)
		err = tmpls.ExecuteTemplate(buff, "pages/app-account", tmplData)
		if err != nil {
			panic(err)
		}
		sse.PatchElements(buff.String())
	}
}
