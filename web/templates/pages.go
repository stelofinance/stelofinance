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

type PageAppAccount struct {
	OnlyRenderPage bool
	AccountId      int64
	Address        string
	LedgerName     string
	IsAdmin        bool
	IsPrimary      bool
	UserId         int64
	Users          []PageAppAccountUser
	TotalTokens    int
	Token          string
}

type PageAppAccountUser struct {
	UserId   int64
	APId     int64
	Username string
}

type PageAppTransfers struct {
	OnlyRenderPage  bool
	RecipientInput  PageAppTransfersRecipientInput
	SelectedAccount PageAppTransfersSelectedAccount
	Accounts        []PageAppTransfersAccount
	Transfers       []PageAppTransfersTransfer
}

type PageAppTransfersRecipientInput struct {
	RecipientLabel  string
	RecipientAddrId int64
	Recipients      []PageAppTransfersRecipients
}

type PageAppTransfersRecipients struct {
	AccountId int64
	Label     string
}

type PageAppTransfersSelectedAccount struct {
	Id         int64
	LedgerName string
	Step       float64
	Balance    float64
}

type PageAppTransfersAccount struct {
	Id    int64
	Label string
}

type PageAppTransfersTransfer struct {
	Id          int64
	Received    bool
	DisplayTime string
	From        string
	To          string
	QtyFmtd     string
	LedgerName  string
	Memo        string
}
