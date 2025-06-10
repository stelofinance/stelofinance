package templates

// layouts/primary

type DataLayoutPrimary struct {
	NavData    DataComponentNav
	FooterData DataComponentFooter
	PageData   any
}
type DataComponentNav struct{}

type DataComponentFooter struct {
	Links []DataComponentFooterLink
}
type DataComponentFooterLink struct {
	Href string
	Text string
}

// layouts/app

type DataLayoutApp struct {
	NavData     DataComponentAppNav
	MenuData    DataComponentAppMenu
	Title       string
	Description string
	PageData    any
}
type DataComponentAppNav struct {
	WalletAddr   string
	ProfileImage string
	Username     string
}
type DataComponentAppMenu struct {
	ActivePage string
	WalletAddr string
}

// pages/wallet-home

type DataPageWalletHomepage struct {
	WalletAddr   string
	SteloSummary DataComponentSteloSummary
}
type DataComponentSteloSummary struct {
	FeaturedAsset    string
	FeaturedAssetQty float64
}

// pages/wallet-settings
type DataPageWalletSettings struct {
	OnlyRenderPage bool
	WalletAddr     string
	Users          []DataPageWalletSettingsUser
}
type DataPageWalletSettingsUser struct {
	Name        string
	IsUser      bool
	Permissions []string
}

// pages/wallet-user-settings
type DataPageWalletUserSettings struct {
	OnlyRenderPage bool
	WalletAddr     string
	Username       string
	Perms          []string
	EnabledPerms   []string
}

// pages/wallet-home

type DataPageWalletAssets struct {
	WalletAddr   string
	SteloSummary DataComponentSteloSummary
	Assets       DataComponentAssets
}
type DataComponentAssets struct {
	Assets []DataComponentAssetAsset
}
type DataComponentAssetAsset struct {
	Name string
	Qty  float64
}

// pages/transact
type DataPageWalletTransact struct {
	OnlyRenderPage       bool
	WalletAddr           string
	TxType               string
	TxRecipient          string
	TxWarehouse          string
	TxNCoord             int
	TxECoord             int
	RecipientSuggestions []DataRecipientSuggestion
	WarehouseSuggestions []DataWarehouseSuggestion
	Assets               []DataTransactAsset
	AllAssets            []DataTransactAsset
}
type DataTransactAsset struct {
	LedgerId int64
	Name     string
}
type DataRecipientSuggestion struct {
	Type       string
	Value      string
	WalletAddr string
}
type DataWarehouseSuggestion struct {
	Label      string
	WalletAddr string
}

// pages/wallet-transactions
type DataPageWalletTransactions struct {
	OnlyRenderPage bool
	WalletAddr     string
	Transactions   []DataTransaction
}
type DataTransaction struct {
	Direction string
	Recipient string
	Timestamp string
	Memo      string
	Assets    []struct {
		Name string
		Qty  float64
	}
}

// pages/wallets
type DataPageWallets struct {
	Wallets []DataPageWalletsWallet
}
type DataPageWalletsWallet struct {
	Addr       string
	IsPersonal bool
	IsAdmin    bool
}

// pages/homepage

type DataPageHomepage struct {
	User      bool
	InfoCards []DataPageHomepageInfoCard
}
type DataPageHomepageInfoCard struct {
	Title string
	Body  string
}
