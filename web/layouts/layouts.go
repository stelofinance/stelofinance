package layouts

import (
	"github.com/stelofinance/stelofinance/internal/assets"
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
		},
		Body: []Node{Class("bg-neutral-900 font-source-code-pro"),
			nav(),
			Group(children),
			footer(),
		},
	})
}

func nav() Node {
	return Nav(
		Class("sticky top-0 z-30 flex items-center gap-3 bg-neutral-900 py-2 px-3  text-white lg:py-4 lg:px-10 2xl:px-20"),
		Aria("label", "Main"),
		A(Href("/"), Aria("label", "Go to homepage"),
			helpers.RenderSVG("icons/LogoFull.html", "w-28 h-auto mr-auto 2xl:w-32"),
		),
		StyleEl(Text(`
			* {
				--header-offset: 61.3px;
			}

			@media (min-width: 1024px) {
				* {
					--header-offset: 77.3px;
				}
			}

			@media (min-width: 1536px) {
				* {
					--header-offset: 83.76px;
				}
			}	
		`)),
	)
}

func footer() Node {
	type link struct {
		Href string
		Text string
	}
	links := []link{
		// { //
		// 	Href: "https://docs.stelo.finance",
		// 	Text: "Docs",
		// },
		{
			Href: "https://discord.gg/t6gM7v7V7T",
			Text: "Discord",
		}, {
			Href: "https://github.com/stelofinance",
			Text: "GitHub",
		}}

	return Footer(
		Class("flex items-center gap-4 py-4 px-3 text-white lg:gap-10 lg:py-6 lg:px-16 2xl:gap-12"),
		helpers.RenderSVG("icons/LogoFull.html", "w-24 h-auto mr-auto lg:w-32 2xl:w-36"),
		Map(links, func(l link) Node {
			return A(
				Class("text-sm lg:text-base 2xl:text-lg"),
				Href(l.Href),
				Rel("noopener noreferrer"),
				Target("_blank"),
				Text(l.Text),
			)
		}),
	)
}
