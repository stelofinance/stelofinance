package templates

import (
	_ "embed"

	"github.com/stelofinance/stelofinance/internal/accounts"
	"github.com/tylermmorton/tmpl"
)

//go:embed pages/index.html.tmpl
var tmplPageIndex string

type PageIndex struct {
	IsAuthed  bool
	InfoCards []PageIndexInfoCard
}
type PageIndexInfoCard struct {
	Title string
	Body  string
}

func (PageIndex) TemplateText() string { return tmplPageIndex }

var Index = tmpl.MustCompile(&LayoutPrimary[PageIndex]{})

//go:embed pages/login.html.tmpl
var tmplPageLogin string

type PageLogin struct {
	Code string
}

func (PageLogin) TemplateText() string { return tmplPageLogin }

// Login is the full-page login render. For SSE partials use
// Login.Render(..., tmpl.WithTarget("page-content")).
var Login = tmpl.MustCompile(&LayoutPrimary[PageLogin]{})

//go:embed pages/app-home.html.tmpl
var tmplPageAppHome string

type PageAppHome struct {
	Username string
}

func (PageAppHome) TemplateText() string { return tmplPageAppHome }

var AppHome = tmpl.MustCompile(&LayoutPrimary[PageAppHome]{})

//go:embed pages/app-accounts.html.tmpl
var tmplPageAppAccounts string

type PageAppAccounts struct {
	Accounts []PageAppAccountsAccount
	Ledgers  []PageAppAccountsLedger
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

func (PageAppAccounts) TemplateText() string { return tmplPageAppAccounts }

var AppAccounts = tmpl.MustCompile(&LayoutPrimary[PageAppAccounts]{})

//go:embed pages/app-account.html.tmpl
var tmplPageAppAccount string

type PageAppAccount struct {
	AccountId   int64
	Address     string
	LedgerName  string
	IsAdmin     bool
	IsPrimary   bool
	UserId      int64
	Users       []PageAppAccountUser
	TotalTokens int
	Token       string
}
type PageAppAccountUser struct {
	UserId   int64
	APId     int64
	Username string
}

func (PageAppAccount) TemplateText() string { return tmplPageAppAccount }

var AppAccount = tmpl.MustCompile(&LayoutPrimary[PageAppAccount]{})

//go:embed pages/app-transfers.html.tmpl
var tmplPageAppTransfers string

type PageAppTransfers struct {
	IdempotencyKey  string
	RecipientInput  ComponentTransferRecipient `tmpl:"components/transfer-recipient"`
	SelectedAccount PageAppTransfersSelectedAccount
	Accounts        []PageAppTransfersAccount
	Transfers       []PageAppTransfersTransfer
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

func (PageAppTransfers) TemplateText() string { return tmplPageAppTransfers }

var AppTransfers = tmpl.MustCompile(&LayoutPrimary[PageAppTransfers]{})

//go:embed pages/app-request.html.tmpl
var tmplPageAppRequest string

type PageAppRequest struct {
	IdempotencyKey string
	LedgerName     string
	AmountFmtd     string
	Amount         int64
	RecipientFmtd  string
	Recipient      int64
	Memo           string
	PrimaryAccount PageAppRequestAccount
	Accounts       []PageAppRequestAccount
}

type PageAppRequestAccount struct {
	Id      int64
	Address string
}

func (PageAppRequest) TemplateText() string { return tmplPageAppRequest }

var AppRequest = tmpl.MustCompile(&LayoutPrimary[PageAppRequest]{})
