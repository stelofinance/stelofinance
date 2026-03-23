package templates

import (
	"github.com/stelofinance/stelofinance/internal/accounts"
)

type PageIndex struct {
	IsAuthed  bool
	InfoCards []PageIndexInfoCard
}
type PageIndexInfoCard struct {
	Title string
	Body  string
}

type PageLogin struct {
	OnlyRenderPage bool
	Code           string
}

type PageAppHome struct {
	Username string
}

type PageAppAccounts struct {
	OnlyRenderPage bool
	Accounts       []PageAppAccountsAccount
	Ledgers        []PageAppAccountsLedger
}

type PageAppAccountsLedger struct {
	ID   int64
	Name string
}
type PageAppAccountsAccount struct {
	AccId      int64
	Addr       string
	IsPrimary  bool
	AccCode    accounts.AccountCode
	LedgerCode accounts.LedgerCode
	LedgerName string
	DisplayQty string
}
