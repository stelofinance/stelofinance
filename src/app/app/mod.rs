//! Authenticated app surface (`/app` and nested routes).
//!
//! Gate: [`require_user`] in this layout. Chrome: Option A — desktop top nav,
//! mobile logo+username top + bottom destinations (see `chrome.rs`).

mod accounts;
mod activity;
mod chrome;
mod deposit;
mod me;
mod transfer;
mod withdraw;

use crate::auth::require_user;
use chrome::{app_bottom_nav, app_header};
use topcoat::{
	Result,
	context::Cx,
	router::{layout, page},
	view::view,
};

/// Wraps every page under `/app`. Requires auth; renders app chrome (not marketing).
#[layout]
async fn app_layout(cx: &Cx, slot: Result) -> Result {
	let user = require_user(cx).await?;

	view! {
		<div class="flex min-h-dvh flex-col bg-neutral-900 pb-16 md:pb-0">
			app_header(username: user.bitcraft_username.clone())
			<div class="flex-1">
				(slot?)
			</div>
			app_bottom_nav()
		</div>
	}
}

/// `GET /app` — thin welcome until a real dashboard exists.
#[page]
async fn home(cx: &Cx) -> Result {
	let user = require_user(cx).await?;

	view! {
		<main class="flex flex-col items-center justify-center px-4 py-16 text-center text-white">
			<p class="text-lg md:text-xl">
				"Hey " (user.bitcraft_username.clone()) ", "
				<span class="text-nowrap">"welcome to Stelo Finance ^-^"</span>
			</p>
			<p class="mt-4 max-w-sm text-sm text-neutral-400">
				"Use Accounts to manage wallets, Activity for history, and Transfer to send."
			</p>
			<div class="mt-8 flex flex-wrap justify-center gap-3 text-sm">
				<a
					href="/app/accounts"
					class="rounded-md bg-anakiwa-700 px-4 py-2 text-white hover:bg-anakiwa-600"
				>
					"Accounts"
				</a>
				<a
					href="/app/transfer"
					class="rounded-md border border-neutral-600 px-4 py-2 text-neutral-200 hover:border-neutral-400"
				>
					"Transfer"
				</a>
			</div>
		</main>
	}
}
