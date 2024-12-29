package layouts

import (
	"github.com/stelofinance/stelofinance/internal/assets"
	"github.com/stelofinance/stelofinance/web/components"
	"github.com/stelofinance/stelofinance/web/helpers"
	. "maragu.dev/gomponents"
	. "maragu.dev/gomponents/components"
	. "maragu.dev/gomponents/html"
)

func Default(children ...Node) Node {
	return HTML5(HTML5Props{
		Title:       "Stelo Finance",
		Description: "A finance platform for the game BitCraft. Free for any to use or even build apps on through our API.",
		Language:    "en",
		Head: []Node{
			Link(Rel("icon"), Href(assets.GetHashedAssetPath("/assets/favicon.png"))),
			// Opt to inline tailwind for now, maybe don't once there is more pages
			// Link(Rel("stylesheet"), Href(assets.GetHashedAssetPath("/assets/tw-output.css"))),
			StyleEl(helpers.RenderStaticToRaw("tw-output.css")),
			helpers.RenderLibHTML("posthog/PostHog_Script.html"),
			Script(Type("module"), Src("/assets/datastar-0-21-3-6638864d4ed0ee19.js")),
		},
		Body: []Node{Class("bg-neutral-900 font-source-code-pro"),
			components.Navbar(components.NavCfg{LogoHref: "/", LogoLabel: "Go to homepage"}),
			Group(children),
			components.Foot(),
		},
	})
}

// func DefaultApp(children ...Node) Node {
// 	return HTML5(HTML5Props{
// 		Title:       "Stelo Finance",
// 		Description: "A finance platform for the game BitCraft. Free for any to use or even build apps on through our API.",
// 		Language:    "en",
// 		Head: []Node{
// 			Link(Rel("icon"), Href(assets.GetHashedAssetPath("/assets/favicon.png"))),
// 			// Opt to inline tailwind for now, maybe don't once there is more pages
// 			// Link(Rel("stylesheet"), Href(assets.GetHashedAssetPath("/assets/tw-output.css"))),
// 			StyleEl(helpers.RenderStaticToRaw("tw-output.css")),
// 			helpers.RenderLibHTML("posthog/PostHog_Script.html"),
// 		},
// 		Body: []Node{Class("bg-neutral-900 font-source-code-pro"),
// 			components.Navbar(components.NavSettings{LogoHref: "/", LogoLabel: "Go to homepage"}),
// 			Group(children),
// 		},
// 	})
// }
