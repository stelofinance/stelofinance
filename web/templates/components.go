package templates

import (
	_ "embed"

	"github.com/tylermmorton/tmpl"
)

//go:embed components/nav.html.tmpl
var tmplComponentNav string

type ComponentNav struct{}

func (*ComponentNav) TemplateText() string { return tmplComponentNav }

//go:embed components/footer.html.tmpl
var tmplComponentFooter string

type ComponentFooter struct {
	Links []ComponentFooterLink
}
type ComponentFooterLink struct {
	Href string
	Text string
}

func (*ComponentFooter) TemplateText() string { return tmplComponentFooter }

//go:embed components/app-nav.html.tmpl
var tmplComponentAppNav string

type ComponentAppNav struct {
	Username string
}

func (*ComponentAppNav) TemplateText() string { return tmplComponentAppNav }

//go:embed components/app-menu.html.tmpl
var tmplComponentAppMenu string

type ComponentAppMenu struct {
	ActivePage string
}

func (*ComponentAppMenu) TemplateText() string { return tmplComponentAppMenu }

//go:embed components/transfer-recipient.html.tmpl
var tmplComponentTransferRecipient string

// ComponentTransferRecipient is the transfers form recipient fieldset.
// Also compiled standalone for Datastar patches of #recipient-input.
type ComponentTransferRecipient struct {
	RecipientLabel  string
	RecipientAddrId int64
	Recipients      []TransferRecipientOption
}

type TransferRecipientOption struct {
	AccountId int64
	Label     string
}

func (*ComponentTransferRecipient) TemplateText() string { return tmplComponentTransferRecipient }

// Standalone compile for Datastar SSE patches of #recipient-input.
var TransferRecipientTmpl = tmpl.MustCompile(&ComponentTransferRecipient{})
