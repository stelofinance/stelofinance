package templates

type LayoutPrimary struct {
	NavData    ComponentNav
	FooterData ComponentFooter
	PageData   any
}

var DefaultLayoutPrimary = LayoutPrimary{
	NavData: ComponentNav{},
	FooterData: ComponentFooter{
		Links: []ComponentFooterLink{{
			Href: "https://discord.gg/t6gM7v7V7T",
			Text: "Discord",
		}, {
			Href: "https://github.com/stelofinance/stelofinance/tree/main/docs",
			Text: "Docs",
		}, {
			Href: "https://github.com/stelofinance",
			Text: "GitHub",
		}},
	},
}

type LayoutApp struct {
	NavData     ComponentAppNav
	MenuData    ComponentAppMenu
	Title       string
	Description string
	PageData    any
}
