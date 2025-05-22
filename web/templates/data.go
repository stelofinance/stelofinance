package templates

type DataLayoutPrimary struct {
	NavData    DataComponentNav
	FooterData DataComponentFooter
	PageData   any
}

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

type DataComponentNav struct{}

type DataComponentFooter struct {
	Links []DataComponentFooterLink
}

type DataComponentFooterLink struct {
	Href string
	Text string
}

type DataPageHomepage struct {
	User      bool
	InfoCards []DataPageHomepageInfoCard
}

type DataPageHomepageInfoCard struct {
	Title string
	Body  string
}
