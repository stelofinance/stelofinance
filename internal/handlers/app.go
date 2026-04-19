package handlers

import (
	"bytes"
	"context"
	"database/sql"
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
	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
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

func loadAppAccountPageData(ctx context.Context, db *database.Database, sessionsKV jetstream.KeyValue, uData *sessions.UserData, accId int64, onlyPage bool) (templates.LayoutApp, error) {
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

	// Count tokens
	keyLstnr, err := sessionsKV.ListKeysFiltered(ctx, "accounts."+strconv.Itoa(int(accId))+".sessions.*")
	if err != nil {
		return templates.LayoutApp{}, err
	}
	defer keyLstnr.Stop()
	tknQty := 0

	for range keyLstnr.Keys() {
		tknQty++
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
			TotalTokens:    tknQty,
		},
	}, nil
}

func AppAccount(tmpls *templates.Tmpls, db *database.Database, sessionsKV jetstream.KeyValue) http.HandlerFunc {
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
			sessionsKV,
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

func PutAccountUser(tmpls *templates.Tmpls, db *database.Database, sessionsKV jetstream.KeyValue) http.HandlerFunc {
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
			sessionsKV,
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

func PostAccountUser(tmpls *templates.Tmpls, db *database.Database, sessionsKV jetstream.KeyValue) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		accId, err := strconv.Atoi(chi.URLParam(r, "account_id"))
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

		// setup qtx
		qtx, err := db.QTx(r.Context(), database.WithForeignKeys)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer qtx.Cleanup()

		// Query the userid from username, then add perms
		usr, err := qtx.Q().GetUserByUsername(r.Context(), body.Username)
		if err != nil {
			if errors.Is(err, sql.ErrNoRows) {
				// TODO: Update page with not found error
				w.WriteHeader(http.StatusNotFound)
				return
			}
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		now := time.Now()

		qtx.Q().InsertAccountPerm(r.Context(), gensql.InsertAccountPermParams{
			AccountID:   int64(accId),
			UserID:      usr.ID,
			Permissions: int64(accounts.PermAdmin),
			UpdatedAt:   now,
			CreatedAt:   now,
		})

		qtx.Commit()

		// Update page
		tmplData, err := loadAppAccountPageData(
			r.Context(),
			db,
			sessionsKV,
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

func DeleteAccountUser(tmpls *templates.Tmpls, db *database.Database, sessionsKV jetstream.KeyValue) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		accId, err := strconv.Atoi(chi.URLParam(r, "account_id"))
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		userId, err := strconv.Atoi(chi.URLParam(r, "user_id"))
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		// setup qtx
		qtx, err := db.QTx(r.Context(), database.WithForeignKeys)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer qtx.Cleanup()

		// Remove user's perms from account
		err = qtx.Q().DeleteAccountPerm(r.Context(), gensql.DeleteAccountPermParams{
			AccountID: int64(accId),
			UserID:    int64(userId),
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
			sessionsKV,
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

func PostAccountToken(tmpls *templates.Tmpls, db *database.Database, sessionsKV jetstream.KeyValue) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		accId, err := strconv.Atoi(chi.URLParam(r, "account_id"))
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		// Create token
		sid := uniuri.NewLen(27)
		sData := sessions.AccountData{
			Id: int64(accId),
		}
		bitties, err := json.Marshal(sData)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		_, err = sessionsKV.Create(r.Context(), "accounts."+strconv.Itoa(accId)+".sessions."+sid, bitties)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		// Update page
		tmplData, err := loadAppAccountPageData(
			r.Context(),
			db,
			sessionsKV,
			uData,
			int64(accId),
			true,
		)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		sse := datastar.NewSSE(w, r)

		// Add token data
		pageData, ok := tmplData.PageData.(templates.PageAppAccount)
		if !ok {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pageData.Token = "stla_" + sid
		tmplData.PageData = pageData

		buff := new(bytes.Buffer)
		err = tmpls.ExecuteTemplate(buff, "pages/app-account", tmplData)
		if err != nil {
			panic(err)
		}
		sse.PatchElements(buff.String())
	}
}

func DeleteAccountTokens(tmpls *templates.Tmpls, db *database.Database, sessionsKV jetstream.KeyValue) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		accId, err := strconv.Atoi(chi.URLParam(r, "account_id"))
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		// Delete all tokens
		keyLstnr, err := sessionsKV.ListKeysFiltered(r.Context(), "accounts."+strconv.Itoa(int(accId))+".sessions.*")
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer keyLstnr.Stop()
		for key := range keyLstnr.Keys() {
			sessionsKV.Delete(r.Context(), key)
		}

		// Update page
		tmplData, err := loadAppAccountPageData(
			r.Context(),
			db,
			sessionsKV,
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

func derefOrFallback[T any](ref *T, fallback T) T {
	if ref != nil {
		return *ref
	}
	return fallback
}

func AppTransfers(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		type input struct {
			AccountId *int64 `json:"accId"`
		}
		var ds input
		if r.URL.Query().Has("datastar") {
			err := json.Unmarshal([]byte(r.URL.Query().Get("datastar")), &ds)
			if err != nil {
				w.WriteHeader(http.StatusBadRequest)
				return
			}
		}

		uData := sessions.GetUser(r.Context())

		// Fetch data
		accsResult, err := db.Q.GetAccountsUserHasPerms(r.Context(), uData.Id)
		if err != nil {
			// TODO
		}
		// If there is ds input, filter by that
		var accId *int64
		if ds.AccountId != nil && *ds.AccountId != -1 {
			accId = ds.AccountId
		}
		transferResult, err := db.Q.GetTransfersUserHasPermsOn(r.Context(), gensql.GetTransfersUserHasPermsOnParams{
			UserID:    uData.Id,
			AccountID: accId,
		})
		if err != nil {
			// TODO
		}

		pageData := templates.PageAppTransfers{}
		if ds.AccountId == nil || *ds.AccountId == -1 {
			pageData.SelectedAccount.Id = -1
		} else {
			// Get all the account info
			pageData.SelectedAccount.Id = *ds.AccountId
			accResult, err := db.Q.GetAccountAndLedgerById(r.Context(), *ds.AccountId)
			if err != nil {
				// TODO
			}
			pageData.SelectedAccount.LedgerName = accResult.LedgerName
			var bal int64 = 0
			if accounts.AccountCode(accResult.Code).IsDebit() {
				bal = accResult.DebitsPosted - accResult.CreditsPosted - accResult.CreditsPending
			} else {
				bal = accResult.CreditsPosted - accResult.DebitsPosted - accResult.DebitsPending

			}
			pageData.SelectedAccount.Balance = float64(bal) / math.Pow(10, float64(accResult.AssetScale))
			pageData.SelectedAccount.Step = 1.0 / math.Pow(10, float64(accResult.AssetScale))
		}

		// Transform data
		pageData.Accounts = make([]templates.PageAppTransfersAccount, 0, len(accsResult))
		for _, acc := range accsResult {
			pageData.Accounts = append(pageData.Accounts, templates.PageAppTransfersAccount{
				Id:    acc.ID,
				Label: "#" + acc.Address + "/" + acc.LedgerName,
			})
		}
		existingTransfers := make(map[int64]struct{})
		pageData.Transfers = make([]templates.PageAppTransfersTransfer, 0, len(transferResult))
		for _, trn := range transferResult {
			data := templates.PageAppTransfersTransfer{
				Id:          trn.ID,
				Received:    false,
				DisplayTime: "",
				From:        "",
				To:          "",
				QtyFmtd:     "",
				LedgerName:  trn.LedgerName,
				Memo:        "",
			}

			_, bothWays := existingTransfers[trn.ID]
			senderId, receiverId := accounts.DetermineSenderReceiver(accounts.TrCode(trn.Code), trn.CreditAccountID, trn.DebitAccountID)
			// If we're filtering by account (via DS), then we shouldn't have both ways,
			// so filter out whichever side this account wasn't on
			if bothWays && ds.AccountId != nil && *ds.AccountId != -1 {
				oldLen := len(pageData.Transfers)
				newTransfers := slices.DeleteFunc(pageData.Transfers, func(t templates.PageAppTransfersTransfer) bool {
					return t.Received == (senderId == *ds.AccountId) && t.Id == trn.ID
				})
				if len(newTransfers) == oldLen {
					continue
				}
				pageData.Transfers = newTransfers
			}

			if !bothWays {
				data.Received = slices.ContainsFunc(accsResult, func(a gensql.GetAccountsUserHasPermsRow) bool {
					return a.ID == receiverId
				})
			}

			data.DisplayTime = humanize.RelTime(time.Now(), trn.CreatedAt, "N/A", "ago")

			if senderId == trn.CreditAccountID {
				data.From = derefOrFallback(trn.CreditUsername, "#"+trn.CreditAddr)
				if !data.Received && !bothWays {
					data.To = derefOrFallback(trn.DebitUsername, "#"+trn.DebitAddr)
				} else {
					data.To = "#" + trn.DebitAddr
				}
			} else {
				data.From = derefOrFallback(trn.DebitUsername, "#"+trn.DebitAddr)
				data.To = derefOrFallback(trn.CreditUsername, "#"+trn.CreditAddr)
			}

			data.QtyFmtd = humanize.Commaf(float64(trn.Amount) / math.Pow(10, float64(trn.AssetScale)))

			if trn.Memo != nil {
				data.Memo = *trn.Memo
			}
			pageData.Transfers = append(pageData.Transfers, data)
			existingTransfers[trn.ID] = struct{}{}
		}

		tmplData := templates.LayoutApp{
			Title:       "Transfers",
			Description: "All transfers on your accounts or selected account",
			NavData: templates.ComponentAppNav{
				Username: uData.BitCraftUsername,
			},
			MenuData: templates.ComponentAppMenu{
				ActivePage: "transfers",
			},
			PageData: pageData,
		}

		if ds.AccountId != nil {
			pageData.OnlyRenderPage = true
			tmplData.PageData = pageData
			sse := datastar.NewSSE(w, r)

			buff := new(bytes.Buffer)
			err = tmpls.ExecuteTemplate(buff, "pages/app-transfers", tmplData)
			if err != nil {
				panic(err)
			}
			sse.PatchElements(buff.String())
			return
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/app-transfers", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

func FormRecipient(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		type input struct {
			AccountId       *int64 `json:"accId"`
			RecipientAccId  *int64 `json:"recipientAccId"`
			RecipientSearch string `json:"recipientSearch"`
		}
		var ds input
		if !r.URL.Query().Has("datastar") {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		err := json.Unmarshal([]byte(r.URL.Query().Get("datastar")), &ds)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		// uData := sessions.GetUser(r.Context())

		// If recipient selected, merge in that
		if ds.RecipientAccId != nil && *ds.RecipientAccId != -1 {
			// Fetch account for Label
			acc, err := db.Q.GetAccountWithUsernameById(r.Context(), *ds.RecipientAccId)
			if err != nil {
				// TODO: Not always just an internal error
				w.WriteHeader(http.StatusInternalServerError)
				return
			}
			label := "#" + acc.Address
			if acc.BitcraftUsername != nil {
				label = *acc.BitcraftUsername
			}
			data := templates.PageAppTransfersRecipientInput{
				RecipientLabel:  label,
				RecipientAddrId: *ds.RecipientAccId,
			}

			sse := datastar.NewSSE(w, r)
			buff := new(bytes.Buffer)
			err = tmpls.ExecuteTemplate(buff, "components/transfer-recipient", data)
			if err != nil {
				panic(err)
			}
			sse.PatchElements(buff.String())
			return
		}

		// If empty, just patch empty
		if ds.AccountId == nil || ds.RecipientSearch == "" {
			sse := datastar.NewSSE(w, r)
			buff := new(bytes.Buffer)
			err = tmpls.ExecuteTemplate(buff, "components/transfer-recipient", nil)
			if err != nil {
				panic(err)
			}
			sse.PatchElements(buff.String())
			return
		}

		// Fetch account for ledger
		acc, err := db.Q.GetAccountById(r.Context(), *ds.AccountId)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		// Fetch the options now
		results, err := db.Q.SearchAccountsByAddrAndUsername(r.Context(), gensql.SearchAccountsByAddrAndUsernameParams{
			SearchTerm:       "%" + strings.ToUpper(ds.RecipientSearch) + "%",
			LedgerID:         acc.LedgerID,
			ExcludeAccountID: *ds.AccountId,
			Limit:            5,
		})
		if err != nil {
			// TODO: Not always an internal server error tbf
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		data := templates.PageAppTransfersRecipientInput{
			RecipientLabel:  "",
			RecipientAddrId: 0,
			Recipients:      make([]templates.PageAppTransfersRecipients, 0, len(results)),
		}
		for _, r := range results {
			label := "#" + r.Address
			if r.BitcraftUsername != nil {
				label = *r.BitcraftUsername
			}
			data.Recipients = append(data.Recipients, templates.PageAppTransfersRecipients{
				AccountId: r.ID,
				Label:     label,
			})
		}

		// Merge in recipients
		sse := datastar.NewSSE(w, r)
		buff := new(bytes.Buffer)
		err = tmpls.ExecuteTemplate(buff, "components/transfer-recipient", data)
		if err != nil {
			panic(err)
		}
		sse.PatchElements(buff.String())
	}
}

func SubmitTransfer(tmpls *templates.Tmpls, db *database.Database, nc *nats.Conn) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		// TODO: Honestly, have the whole entire thing just be form data, then merge
		// in a fragment of success. Or maybe fail message too?
		//
		// Be sure to check user actually owns the account they're submitting from
		// uData := sessions.GetUser(r.Context())

		err := r.ParseForm()
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		accId, err := strconv.ParseInt(chi.URLParam(r, "account_id"), 10, 64)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		recipientId, err := strconv.ParseInt(r.FormValue("recipientId"), 10, 64)
		if err != nil || recipientId == accId {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		var memo *string
		if r.FormValue("memo") != "" {
			str := r.FormValue("memo")
			memo = &str
		}

		acc, err := db.Q.GetAccountAndLedgerById(r.Context(), accId)

		qtyFloat, err := strconv.ParseFloat(r.FormValue("qty"), 64)
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		qtyInt := int64(qtyFloat * math.Pow(10, float64(acc.AssetScale)))

		qtx, err := db.QTx(r.Context(), database.WithForeignKeys)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		defer qtx.Cleanup()

		_, err = accounts.CreateTransfer(r.Context(), qtx.Q(), nc, accounts.CreateTransferInput{
			SendingId:   accId,
			ReceivingId: recipientId,
			Memo:        memo,
			LedgerId:    acc.LedgerID,
			Amount:      qtyInt,
		})
		if err != nil {
			if errors.Is(err, sql.ErrNoRows) {
				w.WriteHeader(http.StatusNotFound)
				return
			}
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		qtx.Commit()

		sse := datastar.NewSSE(w, r)

		// TODO: Too lazy to not just fetch again...
		newAcc, err := db.Q.GetAccountAndLedgerById(r.Context(), accId)
		var newBal int64 = 0
		if accounts.AccountCode(newAcc.Code).IsDebit() {
			newBal = newAcc.DebitsPosted - newAcc.CreditsPosted - newAcc.CreditsPending
		} else {
			newBal = newAcc.CreditsPosted - newAcc.DebitsPosted - newAcc.DebitsPending

		}
		newBalFmtd := float64(newBal) / math.Pow(10, float64(acc.AssetScale))
		sse.PatchElementf(`<span id="bal" class="text-neutral-300 text-sm">(bal: %v)</span>`, newBalFmtd)
		sse.PatchElements(`
			<div id="transfer-form" class="flex justify-between bg-neutral-800 rounded px-2 pt-2 pb-3">
				<p>Transfer Sent!</p>
				<button class="underline text-anakiwa cursor-pointer" data-on:click="@get('/app/transfers')">Start Over</button>
			</div>
		`)
	}
}
