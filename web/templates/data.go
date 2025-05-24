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
	Title       string
	Description string
	PageData    any
}
type DataComponentAppNav struct {
	WalletAddr   string
	ProfileImage string
	Username     string
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

// pages/homepage

type DataPageHomepage struct {
	User      bool
	InfoCards []DataPageHomepageInfoCard
}
type DataPageHomepageInfoCard struct {
	Title string
	Body  string
}
