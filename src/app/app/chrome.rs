//! Authenticated app chrome: top header + mobile bottom destinations.
//!
//! Desktop/tablet (`md+`): logo · Accounts · Activity · Transfer▾ · username→/app/me  
//! Mobile: top logo + username; bottom Accounts · Activity · Transfer (send only).
//!
//! Transfer hover: browsers do not paint closed `<details>` content via CSS alone
//! (see SO / MDN). Datastar sets `details.open` on mouseenter/leave of the control.

use crate::ui::{activity, logo_full, transfer, wallet};
use topcoat::{
	Result,
	context::Cx,
	icon::{IconData, icon, iconify::iconify_icon},
	router::uri,
	view::{attributes, component, view},
};

/// Lucide chevron for the Transfer `<details>` summary (Iconify, compile-time).
const CHEVRON_DOWN: IconData = iconify_icon!("lucide:chevron-down");

#[derive(Clone, Copy, PartialEq, Eq)]
enum NavSection {
	Accounts,
	Activity,
	Transfer,
	Me,
	Other,
}

fn section_from_path(path: &str) -> NavSection {
	if path == "/app/me" || path.starts_with("/app/me/") {
		NavSection::Me
	} else if path == "/app/accounts" || path.starts_with("/app/accounts/") {
		NavSection::Accounts
	} else if path == "/app/activity" || path.starts_with("/app/activity/") {
		NavSection::Activity
	} else if path == "/app/transfer"
		|| path.starts_with("/app/transfer/")
		|| path == "/app/deposit"
		|| path.starts_with("/app/deposit/")
		|| path == "/app/withdraw"
		|| path.starts_with("/app/withdraw/")
	{
		NavSection::Transfer
	} else {
		NavSection::Other
	}
}

fn desktop_link_class(active: bool) -> &'static str {
	if active {
		"inline-flex items-center border-b-2 border-anakiwa px-2 py-1 text-sm font-medium text-white md:text-base"
	} else {
		"inline-flex items-center border-b-2 border-transparent px-2 py-1 text-sm font-medium text-neutral-400 hover:text-white md:text-base"
	}
}

/// Single seamless underline on the Transfer control (not on label + chevron separately).
fn transfer_control_class(active: bool) -> &'static str {
	if active {
		"relative flex items-center border-b-2 border-anakiwa"
	} else {
		"relative flex items-center border-b-2 border-transparent"
	}
}

fn bottom_link_class(active: bool) -> &'static str {
	if active {
		"flex flex-1 flex-col items-center gap-0.5 rounded-lg px-2 py-1.5 text-xs text-white bg-anakiwa-700/40"
	} else {
		"flex flex-1 flex-col items-center gap-0.5 rounded-lg px-2 py-1.5 text-xs text-neutral-400"
	}
}

/// Sticky top header for all `/app/*` pages.
#[component]
pub async fn app_header(cx: &Cx, #[into] username: String) -> Result {
	let section = section_from_path(uri(cx).path());
	let accounts_on = section == NavSection::Accounts;
	let activity_on = section == NavSection::Activity;
	let transfer_on = section == NavSection::Transfer;
	let me_on = section == NavSection::Me;

	let transfer_label_class = if transfer_on {
		"px-2 py-1 text-sm font-medium text-white md:text-base"
	} else {
		"px-2 py-1 text-sm font-medium text-neutral-400 hover:text-white md:text-base"
	};

	let transfer_summary_class = if transfer_on {
		"flex cursor-pointer list-none items-center py-1 pr-1.5 pl-0.5 text-white [&::-webkit-details-marker]:hidden"
	} else {
		"flex cursor-pointer list-none items-center py-1 pr-1.5 pl-0.5 text-neutral-400 hover:text-white [&::-webkit-details-marker]:hidden"
	};

	let username_class = if me_on {
		"ml-auto max-w-[40vw] truncate text-sm text-white underline-offset-2 hover:underline md:text-base"
	} else {
		"ml-auto max-w-[40vw] truncate text-sm text-neutral-400 hover:text-white md:text-base"
	};

	view! {
		<header
			class="sticky top-0 z-30 border-b border-neutral-800 bg-neutral-900"
			aria-label="App header"
		>
			// Full viewport width; progressive horizontal padding (no max-width).
			<nav class="flex w-full items-center gap-1 px-3 py-2 sm:px-5 md:gap-3 md:px-8 md:py-3 lg:px-12 xl:px-16 2xl:px-24">
				<a href="/app" class="shrink-0" aria-label="Stelo app home">
					logo_full(class: "h-auto w-24 md:w-28")
				</a>

				// Primary destinations — desktop / tablet only
				<div class="hidden items-center gap-1 md:flex md:gap-2">
					<a href="/app/accounts" class=(desktop_link_class(accounts_on))>
						"Accounts"
					</a>
					<a href="/app/activity" class=(desktop_link_class(activity_on))>
						"Activity"
					</a>

					// Transfer split control: border on wrapper (seamless underline).
					// Hover opens via Datastar (set open attribute — CSS alone cannot show closed details content).
					<div
						class=(transfer_control_class(transfer_on))
						data-on:mouseenter="el.querySelector('details').open = true"
						data-on:mouseleave="el.querySelector('details').open = false"
					>
						<a href="/app/transfer" class=(transfer_label_class)>
							"Transfer"
						</a>
						<details class="group/dd relative">
							<summary
								class=(transfer_summary_class)
								aria-label="Transfer menu"
							>
								icon(
									data: CHEVRON_DOWN,
									label: "",
									attrs: attributes! {
										class="size-5 shrink-0 opacity-90 transition-transform group-open/dd:rotate-180"
										aria-hidden="true"
									},
								)
							</summary>
							// pt-1 bridge so the pointer stays inside the control when moving to the menu.
							<div
								class="absolute left-0 top-full z-40 min-w-40 pt-1"
								role="menu"
							>
								<div class="rounded-md border border-neutral-700 bg-neutral-900 py-1 shadow-lg">
									<a
										href="/app/transfer"
										class="block px-3 py-2 text-sm text-neutral-200 hover:bg-neutral-800 hover:text-white"
										role="menuitem"
									>
										"Send"
									</a>
									<a
										href="/app/deposit"
										class="block px-3 py-2 text-sm text-neutral-200 hover:bg-neutral-800 hover:text-white"
										role="menuitem"
									>
										"Deposit"
									</a>
									<a
										href="/app/withdraw"
										class="block px-3 py-2 text-sm text-neutral-200 hover:bg-neutral-800 hover:text-white"
										role="menuitem"
									>
										"Withdraw"
									</a>
								</div>
							</div>
						</details>
					</div>
				</div>

				<a href="/app/me" class=(username_class) title=(username.clone())>
					(username)
				</a>
			</nav>
		</header>
	}
}

/// Compact bottom destinations — mobile only (`md:hidden`).
#[component]
pub async fn app_bottom_nav(cx: &Cx) -> Result {
	let section = section_from_path(uri(cx).path());
	let accounts_on = section == NavSection::Accounts;
	let activity_on = section == NavSection::Activity;
	let transfer_on = section == NavSection::Transfer;

	view! {
		<nav
			class="fixed inset-x-0 bottom-0 z-30 border-t border-neutral-800 bg-neutral-900/95 pb-[env(safe-area-inset-bottom)] backdrop-blur md:hidden"
			aria-label="App destinations"
		>
			<div class="flex w-full items-stretch gap-1 px-2 py-1.5 sm:px-4">
				<a href="/app/accounts" class=(bottom_link_class(accounts_on))>
					wallet(class: "size-6")
					<span>"Accounts"</span>
				</a>
				<a href="/app/activity" class=(bottom_link_class(activity_on))>
					activity(class: "size-6")
					<span>"Activity"</span>
				</a>
				<a href="/app/transfer" class=(bottom_link_class(transfer_on))>
					transfer(class: "size-6")
					<span>"Transfer"</span>
				</a>
			</div>
		</nav>
	}
}

/// Shared stub body for unfinished app pages.
#[component]
pub async fn stub_page(#[into] title: String, #[into] blurb: String) -> Result {
	view! {
		<main class="mx-auto max-w-5xl px-4 py-10 text-center text-white">
			<h1 class="text-2xl font-medium md:text-3xl">(title)</h1>
			<p class="mx-auto mt-3 max-w-md text-sm text-neutral-400 md:text-base">
				(blurb)
			</p>
		</main>
	}
}
