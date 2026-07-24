package templates

import (
	_ "embed"

	"github.com/tylermmorton/tmpl"
)

//go:embed layouts/primary.html.tmpl
var tmplLayoutPrimary string

// Shell values for LayoutPrimary.
const (
	ShellPublic = "public"
	ShellApp    = "app"
)

// LayoutPrimary is the single HTML shell for all pages. Shell selects public
// (marketing nav + footer) vs app (app-nav + bottom menu) chrome.
type LayoutPrimary[T tmpl.TemplateProvider] struct {
	Shell string

	// Public chrome (Shell == ShellPublic)
	Nav    ComponentNav    `tmpl:"components/nav"`
	Footer ComponentFooter `tmpl:"components/footer"`

	// App chrome (Shell == ShellApp)
	AppNav ComponentAppNav  `tmpl:"components/app-nav"`
	Menu   ComponentAppMenu `tmpl:"components/app-menu"`

	Content     T `tmpl:"page-content"`
	Title       string
	Description string
	Env         string
}

// Prepend static icon/illustration defines so they are available everywhere
// under the layout tree without nesting them as TemplateProviders.
func (*LayoutPrimary[T]) TemplateText() string {
	return staticDefines() + tmplLayoutPrimary
}

func (*LayoutPrimary[T]) TemplateFuncMap() tmpl.FuncMap {
	return globalFuncs
}

// DefaultFooter returns the standard marketing footer links.
func DefaultFooter() ComponentFooter {
	return ComponentFooter{
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
	}
}

// PublicLayout builds a marketing/public layout around content.
func PublicLayout[T tmpl.TemplateProvider](content T, env string) *LayoutPrimary[T] {
	return &LayoutPrimary[T]{
		Shell:   ShellPublic,
		Nav:     ComponentNav{},
		Footer:  DefaultFooter(),
		Content: content,
		Env:     env,
	}
}

// AppLayout builds an authenticated app shell around content.
func AppLayout[T tmpl.TemplateProvider](title, description, username, activePage, env string, content T) *LayoutPrimary[T] {
	return &LayoutPrimary[T]{
		Shell:       ShellApp,
		AppNav:      ComponentAppNav{Username: username},
		Menu:        ComponentAppMenu{ActivePage: activePage},
		Title:       title,
		Description: description,
		Content:     content,
		Env:         env,
	}
}
