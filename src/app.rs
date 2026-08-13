use topcoat::{
	Result,
	asset::{AssetBundle, RouterBuilderAssetExt, asset},
	context::Cx,
	cookie::RouterBuilderCookieExt,
	font::{Font, font},
	router::{Router, RouterBuilderDiscoverExt, layout, module_router, page},
	tailwind,
	view::view,
};

mod app;
mod auth;
mod login;

use crate::auth::bitauth::BitAuth;
use crate::auth::cookies::{COOKIE_REFRESH, COOKIE_TOKEN, get_cookie};
use crate::stdb::StdbState;
use crate::ui::{discord, github, logo_colored, nintron, public_footer, public_nav, right_arrow};

const SOURCE_CODE_PRO: Font = font! {
	"Source Code Pro",
	@font-face {
		src: url(asset!("https://cdn.jsdelivr.net/fontsource/fonts/source-code-pro:vf@5.3.0/latin-wght-normal.woff2")) format("woff2") tech("variations");
		font-weight: 200 900;
		font-style: normal;
		font-display: swap;
	}
	@font-face {
		src: url(asset!("https://cdn.jsdelivr.net/fontsource/fonts/source-code-pro:vf@5.3.0/latin-wght-italic.woff2")) format("woff2") tech("variations");
		font-weight: 200 900;
		font-style: italic;
		font-display: swap;
	}
};

/// Build the HTTP router for the lite edge.
///
/// Requires BitAuth env (`BITAUTH_CLIENT_ID`, `BITAUTH_CLIENT_SECRET`,
/// `BITAUTH_REDIRECT_URL`, …); refuses to start if discovery/config fails.
///
/// STDB: `STDB_HOST` + `STDB_DATABASE` (defaults: local standalone + `stelofinance`).
pub async fn router() -> Router {
	let bitauth = BitAuth::from_env()
		.await
		.unwrap_or_else(|e| panic!("BitAuth required to start: {e}"));
	let stdb = StdbState::from_env();
	eprintln!(
		"stdb: host={} database={} (einro token-keyed pool)",
		stdb.config.host, stdb.config.database
	);

	module_router!()
		.discover()
		.cookies()
		.app_context(bitauth)
		.app_context(stdb)
		.assets(
			AssetBundle::load()
				.expect("asset bundle missing — run `topcoat asset bundle` or `topcoat dev`"),
		)
		.build()
}

#[layout]
async fn root_layout(slot: Result) -> Result {
	view! {
		<!DOCTYPE html>
		<html lang="en">
			<head>
				<meta charset="UTF-8">
				<meta name="viewport" content="width=device-width, initial-scale=1.0">
				<title>"Stelo Finance"</title>
				<meta
					name="description"
					content="A finance platform for the game BitCraft. Free for any to use or even build apps on through our API."
				>
				<link rel="icon" href=(asset!("assets/favicon.png"))>
				topcoat::font::link(font: SOURCE_CODE_PRO)
				<link rel="stylesheet" href=(tailwind::stylesheet!())>
				// Datastar client (inline data-* handlers, e.g. Transfer menu hover).
				<script
					type="module"
					src=(asset!("https://cdn.jsdelivr.net/gh/starfederation/datastar@1.0.2/bundles/datastar.js"))
				></script>
				topcoat::dev::script()
			</head>
			<body class="bg-neutral-900 font-source-code-pro text-white">
				// Document shell only. Marketing pages opt into public_nav/footer;
				// `/app/*` uses its own chrome in `app::app` layout.
				(slot?)
			</body>
		</html>
	}
}

/// Marketing homepage — port of Go `handlers.Index` + `pages/index.html.tmpl`.
///
/// Auth only toggles Log In vs Dashboard; no STDB calls on this page.
#[page]
async fn home(cx: &Cx) -> Result {
	let is_authed =
		get_cookie(cx, COOKIE_TOKEN).is_some() || get_cookie(cx, COOKIE_REFRESH).is_some();

	const INTRO: &str = "Stelo keeps BitCraft assets in digital accounts so you can send, receive, and build with them whether you're online or not. Each asset has its own balance; transfers move value between accounts instantly.";

	struct Step {
		number: &'static str,
		title: &'static str,
		body: &'static str,
	}
	const STEPS: &[Step] = &[
		Step {
			number: "01",
			title: "Get assets",
			body: "Bring value onto Stelo through that asset's issuer, or receive a transfer from someone who already holds it.",
		},
		Step {
			number: "02",
			title: "Send them",
			body: "Move balances between accounts instantly. To friends, for settlements, or via payment links. No in-game proximity required.",
		},
		Step {
			number: "03",
			title: "Cash out",
			body: "For redeemable assets, return them to the issuer and take the items back into BitCraft.",
		},
	];

	struct AssetCard {
		name: &'static str,
		issuer: &'static str,
		description: &'static str,
		type_label: &'static str,
	}
	const ASSETS: &[AssetCard] = &[AssetCard {
		name: "Hexcoin",
		issuer: "Stelo Bank",
		description: "BitCraft's hexcoin on Stelo. Deposit and redeem 1:1 through the official bank; transfer freely between accounts.",
		type_label: "In-game item",
	}];

	let cta_href = if is_authed { "/app" } else { "/login" };
	let cta_label = if is_authed { "Dashboard" } else { "Log In" };

	view! {
		public_nav()
		<main>
			<div class="relative flex h-screen-available flex-col items-center justify-center">
				<div class="mb-40 flex flex-col items-center gap-4 text-white lg:flex-row lg:gap-10 2xl:gap-14">
					logo_colored(class: "h-32 w-32 lg:h-48 lg:w-48 2xl:h-64 2xl:w-64")
					<div class="flex flex-col">
						<h1 class="text-center text-2xl font-medium lg:text-4xl 2xl:text-5xl 2xl:leading-tight">
							<a
								class="underline"
								rel="noopener noreferrer"
								target="_blank"
								href="https://bitcraftonline.com/"
							>
								"BitCraft"
							</a>
							"'s leading"
							<br>
							"finance platform"
						</h1>
						<h2 class="mt-2 flex justify-center text-center text-sm text-neutral-100 lg:mt-3 lg:text-base 2xl:text-lg">
							"Player focused."
							"\u{00a0}"
							"Player driven."
						</h2>
						<div class="mt-3 flex justify-center gap-5 lg:mt-5 2xl:gap-8">
							<a
								href=(cta_href)
								class="flex items-center gap-2 rounded-full bg-melrose px-4 py-1 text-sm font-medium text-neutral-800 transition-colors duration-300 hover:bg-melrose-200 lg:px-6 lg:text-base 2xl:px-14 2xl:py-2 2xl:text-lg"
							>
								(cta_label)
							</a>
							<a
								href="https://discord.gg/t6gM7v7V7T"
								rel="noopener noreferrer"
								target="_blank"
								class="flex items-center gap-2 rounded-full border border-white px-4 py-1 text-sm font-medium text-white lg:gap-3 lg:px-6 lg:text-base 2xl:px-14 2xl:py-2 2xl:text-lg"
							>
								"Discord"
								right_arrow(class: "h-3 w-3 lg:h-3.5 lg:w-3.5 2xl:h-4 2xl:w-4")
							</a>
						</div>
					</div>
				</div>
				<div class="absolute bottom-0 flex w-full justify-center bg-linear-to-r from-anakiwa to-melrose py-4 text-neutral-900 2xl:py-5 2xl:text-xl">
					<p class="tracking-wider">"THE STELO FINANCE PLATFORM"</p>
				</div>
			</div>

			<section class="mx-auto max-w-(--breakpoint-xl) px-6 py-16 text-white md:py-20 lg:px-10 lg:py-24">
				<h2 class="bg-linear-to-r from-anakiwa to-melrose bg-clip-text text-2xl font-medium text-transparent sm:text-3xl lg:text-4xl">
					"How Stelo Works"
				</h2>
				<p class="mt-3 max-w-3xl text-sm text-neutral-100 sm:text-base lg:mt-5 lg:text-lg lg:leading-snug">
					(INTRO)
				</p>

				<div class="mt-8 grid grid-cols-1 gap-6 sm:mt-10 sm:grid-cols-3 lg:mt-14 2xl:gap-8">
					for step in STEPS {
						<div class="rounded-lg bg-neutral-950 p-5 sm:p-6">
							<span class="text-sm font-medium text-anakiwa">(step.number)</span>
							<h3 class="mt-1 text-lg font-medium sm:text-xl">(step.title)</h3>
							<p class="mt-2 text-sm text-neutral-100 sm:text-base lg:leading-snug">
								(step.body)
							</p>
						</div>
					}
				</div>

				<div class="mt-14 sm:mt-16 lg:mt-20">
					<h3 class="text-lg font-medium sm:text-xl lg:text-2xl">
						"Assets on the platform"
					</h3>
					<div class="mt-5 flex flex-col gap-4 sm:mt-6">
						for asset in ASSETS {
							<div class="rounded-lg border border-neutral-800 bg-neutral-950 p-5 sm:p-6">
								<div class="flex flex-wrap items-center justify-between">
									<p class="text-lg font-medium sm:text-xl">(asset.name)</p>
									<span class="rounded-full bg-anakiwa/15 px-3 py-0.5 text-xs text-anakiwa sm:text-sm">
										(asset.type_label)
									</span>
								</div>
								<p class="mt-1 text-sm text-neutral-300 sm:text-base">
									<span class="text-neutral-400">"Issuer:"</span>
									" "
									(asset.issuer)
								</p>
								<p class="mt-3 text-sm text-neutral-100 sm:text-base lg:leading-snug">
									(asset.description)
								</p>
							</div>
						}
					</div>
				</div>
			</section>

			<div class="flex w-full justify-center gap-36 bg-linear-to-r from-melrose to-anakiwa px-11 py-6 sm:px-12 lg:px-16 lg:py-10">
				<div class="flex flex-col xl:max-w-(--breakpoint-md)">
					<h2 class="text-2xl font-medium text-neutral-900 lg:text-3xl">
						"Contribute to the Stelo Ecosystem"
					</h2>
					<p class="mt-3 text-neutral-800 lg:mt-5 lg:text-xl lg:leading-tight">
						"Have a great idea for an app on our platform? Or maybe you're looking to directly contribe to Stelo's core functionality? Either way, Stelo thrives on community involvement in it's ecosystem, and we'd love your help!"
					</p>
					<div class="mt-6 flex gap-4 text-sm lg:gap-5 lg:text-base xl:mt-auto">
						<a
							class="flex items-center gap-2 rounded-md bg-neutral-900 px-2 py-1 text-white hover:shadow-md lg:px-3 lg:py-2"
							href="https://github.com/stelofinance"
							rel="noopener noreferrer"
							target="_blank"
						>
							"GitHub"
							github(class: "size-5 lg:size-6")
						</a>
						<a
							class="flex items-center gap-2 rounded-md bg-neutral-900 px-2 py-1 text-white hover:shadow-md lg:px-3 lg:py-2"
							href="https://discord.gg/t6gM7v7V7T"
							rel="noopener noreferrer"
							target="_blank"
						>
							"Join the Discord"
							discord(class: "size-5 lg:size-6")
						</a>
					</div>
				</div>
				nintron(class: "hidden xl:block")
			</div>
		</main>
		public_footer()
	}
}
