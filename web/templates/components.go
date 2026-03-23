package templates

type ComponentNav struct{}

type ComponentFooter struct {
	Links []ComponentFooterLink
}
type ComponentFooterLink struct {
	Href string
	Text string
}

type ComponentAppNav struct {
	Username string
}
type ComponentAppMenu struct {
	ActivePage string
}
