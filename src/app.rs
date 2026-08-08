use topcoat::{
	Result,
	asset::{AssetBundle, RouterBuilderAssetExt, asset},
	cookie::RouterBuilderCookieExt,
	font::{Font, font},
	router::{Router, RouterBuilderDiscoverExt, layout, module_router, page},
	tailwind,
	view::view,
};

mod auth;
mod login;

use crate::auth::bitauth::BitAuth;
use crate::stdb::StdbState;

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
					content="A finance platform for the game BitCraft. Free for any to use or build apps on through our API."
				>
				<link rel="icon" href=(asset!("assets/favicon.png"))>
				topcoat::font::link(font: SOURCE_CODE_PRO)
				<link rel="stylesheet" href=(tailwind::stylesheet!())>
				topcoat::dev::script()
			</head>
			<body class="min-h-dvh bg-neutral-900 font-source-code-pro text-white">
				(slot?)
			</body>
		</html>
	}
}

#[page]
async fn home() -> Result {
	view! {
		<main class="flex min-h-dvh flex-col items-center justify-center gap-4 px-4">
			<p class="text-sm uppercase tracking-widest text-anakiwa">
				"Stelo Finance"
			</p>
			<h1 class="text-center text-3xl font-medium text-melrose lg:text-4xl">
				"Edge skeleton"
			</h1>
			<p class="max-w-lg text-center text-neutral-300">
				"Topcoat layout, BitAuth, einro STDB pool, and token auto-refresh are wired."
			</p>
			<div class="flex flex-wrap items-center justify-center gap-4">
				<a href="/login" class="text-anakiwa underline hover:text-anakiwa-300">
					"Login"
				</a>
			</div>
			<p class="text-sm text-neutral-500">
				"lite webserver · Rust / Topcoat"
			</p>
		</main>
	}
}
