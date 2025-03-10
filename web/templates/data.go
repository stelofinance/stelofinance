package templates

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

type DataPageHomepage struct {
	User      bool
	InfoCards []DataPageHomepageInfoCard
}

type DataPageHomepageInfoCard struct {
	Title string
	Body  string
}
