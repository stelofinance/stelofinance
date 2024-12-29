package pages

import (
	"github.com/stelofinance/stelofinance/web/helpers"
	"github.com/stelofinance/stelofinance/web/layouts"
	. "maragu.dev/gomponents"
	. "maragu.dev/gomponents/html"
)

func Login() Node {
	return layouts.Default(Main(Class("flex flex-col h-screen-available items-center justify-center text-white"),
		H1(Text("Login"), Class("font-medium text-4xl")),
		A(Class("mt-16 mb-40 flex items-center gap-2 rounded-md bg-melrose-800 py-2 px-3 text-white hover:shadow-md lg:text-lg lg:py-3 lg:px-6"),
			Href("/auth/discord"),
			Text("Continue with Discord"),
			helpers.RenderSVG("icons/Discord.html", "size-5 lg:size-6"),
		),
	))
}
