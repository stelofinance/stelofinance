package pages

import (
	"github.com/stelofinance/stelofinance/web/helpers"
	"github.com/stelofinance/stelofinance/web/layouts"
	. "maragu.dev/gomponents"
	. "maragu.dev/gomponents/html"
)

func Homepage() Node {
	helpers.RenderSVG("icons/LogoColored.html", "h-32 w-32 lg:h-48 lg:w-48 2xl:h-64 2xl:w-64")

	return layouts.Default(
		homepageHero(),
		homepageInfoCards(),
		homepageWalkthrough(),
		homepageContribute(),
	)
}

func homepageHero() Node {
	return Main(Class("relative flex h-screen-available flex-col items-center justify-center"),
		Div(Class("mb-40 flex flex-col items-center gap-4 text-white lg:flex-row lg:gap-10 2xl:gap-14"),
			helpers.RenderSVG("icons/LogoColored.html", "h-32 w-32 lg:h-48 lg:w-48 2xl:h-64 2xl:w-64"),
			Div(Class("flex flex-col"),
				H1(Class("text-center text-2xl font-medium lg:text-4xl 2xl:text-5xl 2xl:leading-tight"),
					A(Class("underline"), Rel("noopener noreferrer"), Target("_blank"), Href("https://bitcraftonline.com/"),
						Text("BitCraft"),
					),
					Text("'s leading"),
					Br(),
					Text("finance platform"),
				),
				H2(Class("mt-2 flex justify-center text-center text-sm text-neutral-100 lg:mt-3 lg:text-base 2xl:text-lg"),
					Raw("Innovative.&nbsp;"),
					Span(Class("hidden lg:block"), Raw("Player focused.&nbsp;")),
					Raw("Player driven."),
				),
				Div(Class("mt-3 flex gap-5 lg:mt-5 lg:justify-center 2xl:gap-8"),
					Button(
						Class(`flex items-center gap-2 rounded-full bg-melrose py-1 px-4 text-sm font-medium text-neutral-800
									transition-colors duration-300 cursor-not-allowed hover:bg-melrose-200 lg:px-6 lg:text-base 2xl:py-2 2xl:px-14 2xl:text-lg`),
						Disabled(),
						Text("Coming Soon"),
					),
					A(
						Href("https://discord.gg/t6gM7v7V7T"),
						Rel("noopener noreferrer"),
						Target("_blank"),
						Class(`flex items-center gap-2 rounded-full border border-white py-1 px-4 text-sm font-medium
									text-white lg:gap-3 lg:px-6 lg:text-base 2xl:py-2 2xl:px-14 2xl:text-lg`),
						Text("Discord"),
						helpers.RenderSVG("icons/RightArrow.html", "h-3 w-3 lg:h-3.5 lg:w-3.5 2xl:h-4 2xl:w-4"),
					),
				),
			),
		),
		Div(Class("absolute bottom-0 flex w-full justify-center bg-gradient-to-r from-anakiwa to-melrose py-4 2xl:py-5 2xl:text-xl"),
			P(Class("tracking-wider"), Text("THE STELO FINANCE PLATFORM")),
		),
	)
}

func homepageInfoCards() Node {
	type information struct {
		Title string
		Body  string
	}
	info := []information{{
		Title: "Convienent in every way",
		Body:  "One of Stelo's core goals is to be a convienent way for managing all your finances. Once you've created your account the entire platform is at your fingertips.",
	}, {
		Title: "Connecting the physical to the digial",
		Body:  "Every item in the Stelo ecosystem is backed by the real asset in game. Whenever you want any of your digital goods in game, just visit a Stelo partnered warehouse and you'll receive the items from your account.",
	}, {
		Title: "Built to be built upon",
		Body:  "By leveraging Stelo's app platform you can build loan services, trading bots, tax systems, and so much more! If you're daring enough, you could even build another entire finance platform ontop.",
	}, {
		Title: "A simplistic currency",
		Body:  "The Stelo currency is a divisible, limited supply currency built into the Stelo platform. It's main purpose is to be the collateral against assets stored in Stelo partnered warehouses.",
	}, {
		Title: "A free platform",
		Body:  "Stelo's core functionality is completely free! No monthly subscription, no transactions fees on anything. Stelo will be monetized by other means if needed.",
	}, {
		Title: "A global exchange",
		Body:  "To showcase the power of the smart wallet system, Stelo will be creating a global exchange where users can sell goods to anyone, anytime, anywhere. This utility will be only just the start of the Stelo ecosystem.",
	}}
	return Div(Class("bg-gradient-to-b from-anakiwa-300 to-melrose py-10 px-4 lg:py-16 lg:px-10"),
		Div(Class("mx-auto grid max-w-sm grid-cols-1 gap-4 sm:max-w-screen-xl sm:grid-cols-2 md:gap-6 lg:grid-cols-3 xl:gap-10"),
			Map(info, func(i information) Node {
				return Div(Class("flex flex-col rounded-lg bg-white bg-opacity-20 py-5 px-3 text-center shadow-md xl:p-8"),
					H2(Class("text-lg font-medium xl:text-2xl"), Text(i.Title)),
					P(Class("mt-2 text-sm font-light xl:mt-6 xl:text-lg xl:leading-tight"), Text(i.Body)),
				)
			}),
		),
	)
}

// TODO: Clean up this function
func homepageWalkthrough() Node {
	return Div(Class("mx-auto flex max-w-screen-md flex-col py-10 px-6 text-white md:py-20 xl:max-w-screen-lg xl:py-28"),
		H2(Class("mx-auto text-lg uppercase tracking-wider text-anakiwa sm:text-2xl md:mb-4 lg:text-3xl xl:mb-12 xl:text-4xl"), Text("Getting started on Stelo")),
		Div(Class("group relative flex h-fit justify-between rounded-lg p-8 transition-colors xl:px-9 xl:pb-16 xl:hover:bg-neutral-950"),
			Div(Class("absolute top-10 left-0 h-full w-2 bg-neutral-800")),
			Div(Class("absolute top-10 -left-1.5 h-5 w-5 rounded-full bg-neutral-800 transition-colors group-hover:bg-anakiwa")),
			Div(Class("xl:w-1/2"),
				H3(Class("text-xl font-medium lg:text-2xl"), Text("Create an account")),
				P(Class("mt-5 text-sm text-neutral-100 lg:text-base xl:text-lg"), Text(`
					To use the Stelo platform, you'll first need to create an account. With your account you get
					a personal wallet, think of it like a bank account for any items you want to deposit. You
					can then send these items to anyone else on Stelo.
				`)),
			),
			Div(Class("hidden w-52 xl:block"),
				helpers.RenderSVG("illustrations/Account.html", "grayscale group-hover:grayscale-0"),
			),
		),
		Div(Class("group relative flex h-fit justify-between rounded-lg p-8 transition-colors xl:px-9 xl:pb-16 xl:hover:bg-neutral-950"),
			Div(Class("absolute top-0 left-0 h-full w-2 bg-neutral-800")),
			Div(Class("absolute top-9 -left-1.5 h-5 w-5 rounded-full bg-neutral-800 transition-colors group-hover:bg-anakiwa")),
			Div(Class("xl:w-1/2"),
				H3(Class("text-xl font-medium lg:text-2xl"), Text("Deposit items")),
				P(Class("mt-5 text-sm text-neutral-100 lg:text-base xl:text-lg"), Text(`
					Now that you have an account, you'll need to put items in it to trade with. Head to your
					nearest Stelo partnered warehouse in BitCraft, and deposit your goods with them that you'd
					like on your stelo account.
				`)),
			),
			Div(Class("hidden w-52 xl:block"),
				helpers.RenderSVG("illustrations/Deposit.html", "grayscale group-hover:grayscale-0"),
			),
		),
		Div(Class("group relative flex h-fit justify-between rounded-lg p-8 transition-colors xl:px-9 xl:pb-16 xl:hover:bg-neutral-950"),
			Div(Class("absolute top-0 left-0 h-full w-2 bg-neutral-800")),
			Div(Class("absolute top-9 -left-1.5 h-5 w-5 rounded-full bg-neutral-800 transition-colors group-hover:bg-anakiwa")),
			Div(Class("xl:w-1/2"),
				H3(Class("text-xl font-medium lg:text-2xl"), Text("Trade your items")),
				P(Class("mt-5 text-sm text-neutral-100 lg:text-base xl:text-lg"), Text(`
					With items in your wallet, you can now do whatever you'd like with them on the Stelo
					platform! You can send them to a friend, sell them on an exchange, donate them to a cause,
					and much more!
				`)),
			),
			Div(Class("hidden w-52 xl:block"),
				helpers.RenderSVG("illustrations/Trade.html", "grayscale group-hover:grayscale-0"),
			),
		),
		Div(Class("group relative flex h-fit justify-between rounded-lg p-8 transition-colors xl:px-9 xl:pb-16 xl:hover:bg-neutral-950"),
			Div(Class("absolute top-0 left-0 h-10 w-2 bg-neutral-800")),
			Div(Class("absolute top-9 -left-1.5 h-5 w-5 rounded-full bg-neutral-800 transition-colors group-hover:bg-anakiwa")),
			Div(Class("xl:w-1/2"),
				H3(Class("text-xl font-medium lg:text-2xl"), Text("Withdraw items")),
				P(Class("mt-5 text-sm text-neutral-100 lg:text-base xl:text-lg"), Text(`
					Have some items that you want to withdraw back into the world of BitCraft? Go to your local
					Stelo partnered warehouse and request from them what items you'd like to withdraw from your
					account!
				`)),
			),
			Div(Class("hidden w-60 items-center xl:flex"),
				helpers.RenderSVG("illustrations/Withdraw.html", "grayscale group-hover:grayscale-0"),
			),
		),
	)
}

func homepageContribute() Node {
	return Div(Class("my-10 mx-6 flex max-w-screen-xl gap-36 rounded-lg bg-gradient-to-r from-melrose to-anakiwa p-4 lg:mx-16 lg:py-6 lg:px-10 xl:mx-auto"),
		Div(Class("flex flex-col"),
			H2(Class("text-2xl font-medium lg:text-3xl"), Text("Contribute to the Stelo Ecosystem")),
			P(Class("mt-3 text-neutral-800 lg:mt-5 lg:text-xl lg:leading-tight"), Text(`
				Have a great idea for an app on our platform? Or maybe you're looking to directly contribe to
				Stelo's core functionality? Either way, Stelo thrives on community involvement in it's
				ecosystem, and we'd love your help!
			`)),
			Div(Class("mt-6 flex gap-4 text-sm lg:gap-5 lg:text-base xl:mt-auto"),
				A(Class("flex items-center gap-2 rounded-md bg-neutral-900 py-1 px-2 text-white hover:shadow-md lg:py-2 lg:px-3"),
					Href("https://github.com/stelofinance"),
					Rel("noopener noreferrer"),
					Target("_blank"),
					Text("GitHub"),
					helpers.RenderSVG("icons/GitHub.html", "size-5 lg:size-6"),
				),
				A(Class("flex items-center gap-2 rounded-md bg-neutral-900 py-1 px-2 text-white hover:shadow-md lg:py-2 lg:px-3"),
					Href("https://github.com/stelofinance"),
					Rel("noopener noreferrer"),
					Target("_blank"),
					Text("Join the Discord"),
					helpers.RenderSVG("icons/Discord.html", "size-5 lg:size-6"),
				),
			),
		),
		helpers.RenderSVG("Nintron.html", "hidden xl:block"),
	)
}
