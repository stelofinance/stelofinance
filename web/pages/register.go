package pages

import (
	"github.com/stelofinance/stelofinance/web/components"
	"github.com/stelofinance/stelofinance/web/layouts"
	. "maragu.dev/gomponents"
	. "maragu.dev/gomponents/html"
)

func Register() Node {
	return layouts.Default(Main(Class("flex flex-col h-screen-available items-center justify-center text-white"),
		H1(Text("Register"), Class("font-medium text-4xl")),
		Form(Method("POST"),
			components.InputUnderlined(components.InputCfg{
				Name:  "username",
				Label: "Username",
				Value: "test",
				Class: "stuff",
			}),
			components.InputUnderlined(components.InputCfg{
				Name:     "password",
				IsSecret: true,
				Label:    "Password",
				Value:    "",
				Class:    "stuff",
			}),
		),
	))
}
