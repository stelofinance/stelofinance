//! App list HTML, token sheet, and `POST /app/me/apps/token`.

use crate::auth::require_user;
use crate::module_bindings::AppTicketPurpose;
use crate::stdb::apps::{MyAppsData, mint_app_identity};
use crate::stdb::{
	StdbState, acquire_user_db, create_user_app_ticket, format_rel_time, replace_user_app_ticket,
	unix_now_micros,
};
use serde::{Deserialize, Serialize};
use topcoat::{
	Result,
	context::{Cx, app_context},
	datastar::{PatchSignals, Signals},
	router::route,
	view::{Unescaped, View, component, view},
};

#[component]
pub async fn my_apps_list(data: MyAppsData) -> Result<impl View> {
	Ok(view! {
		(Unescaped::new_unchecked(my_apps_html(&data)))
	})
}

pub fn my_apps_html(data: &MyAppsData) -> String {
	let mut out = String::from(r#"<div id="me-apps" class="flex flex-col">"#);
	out.push_str(r#"<section class="mt-8">"#);
	out.push_str(r#"<div class="flex flex-wrap items-center justify-between gap-3">"#);
	out.push_str(
		r#"<h2 class="text-sm font-medium uppercase tracking-wide text-neutral-400">Apps</h2>"#,
	);
	out.push_str(
		r#"<button type="button" class="cursor-pointer rounded-md bg-anakiwa-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50" data-attr:disabled="$creatingApp || $replacingApp || $appToken != ''" data-on:click="$creatingApp = true; $replacingApp = false; $appError = ''; $appName = ''">New app</button>"#,
	);
	out.push_str("</div>");
	out.push_str(
		r#"<p class="mt-2 text-sm text-neutral-400">Bots you own. Copy the token once, then grant a role on an account under Permissions.</p>"#,
	);
	if data.apps.is_empty() && data.tickets.is_empty() {
		out.push_str(
			r#"<p class="mt-3 text-sm text-neutral-400">No apps yet. Use <span class="text-neutral-200">New app</span> to generate a token.</p>"#,
		);
	}
	if !data.tickets.is_empty() {
		out.push_str(
			r#"<p class="mt-4 text-xs font-medium uppercase tracking-wide text-neutral-500">Open tickets</p>"#,
		);
		out.push_str(r#"<div class="mt-2 flex flex-col gap-2">"#);
		let now = unix_now_micros();
		for t in &data.tickets {
			let purpose = match t.purpose {
				AppTicketPurpose::Create => "Create",
				AppTicketPurpose::Replace => "Replace",
			};
			let left = format_rel_time(t.expires_at.to_micros_since_unix_epoch(), now);
			let name = escape_html(&t.name);
			out.push_str(&format!(
				r#"<div class="rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2.5"><p class="truncate text-sm text-neutral-100">{name}</p><p class="mt-0.5 text-xs text-neutral-400">{purpose} · expires {left}</p></div>"#
			));
		}
		out.push_str("</div>");
	}
	if !data.apps.is_empty() {
		out.push_str(r#"<div class="mt-3 flex flex-col gap-2">"#);
		for a in &data.apps {
			let name = escape_html(&a.name);
			let hex = escape_html(&a.id.to_hex());
			let name_js = escape_html(&js_single(&a.name));
			out.push_str(&format!(
				r#"<div class="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2.5"><div class="min-w-0"><p class="truncate text-sm text-neutral-100">{name}</p><p class="mt-0.5 truncate font-mono text-xs text-neutral-500">{hex}</p></div>"#
			));
			out.push_str(&format!(
				r#"<button type="button" class="cursor-pointer text-sm text-neutral-300 hover:text-white disabled:cursor-not-allowed disabled:opacity-50" data-attr:disabled="$creatingApp || $replacingApp || $appToken != ''" data-on:click="$replacingApp = true; $creatingApp = false; $appError = ''; $appName = {name_js}">Replace</button>"#
			));
			out.push_str("</div>");
		}
		out.push_str("</div>");
	}
	out.push_str("</section></div>");
	out
}

/// New-app form + one-time token sheet (outside live remorph).
#[component]
pub async fn app_forms() -> Result<impl View> {
	Ok(view! {
		<section
			class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
			data-show="$creatingApp && !$appToken"
			style="display: none"
		>
			<h2 class="text-lg font-medium">"New app"</h2>
			<p class="mt-1 text-sm text-neutral-300">
				"Generates a SpacetimeDB identity and token for your bot. Copy the token once. Then grant the app a role on an account."
			</p>
			<input
				type="text"
				class="mt-4 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm placeholder:text-neutral-400"
				data-bind="appName"
				maxlength="64"
				placeholder="App name"
				autocomplete="off"
			>
			<div class="mt-4 flex flex-wrap gap-2">
				<button
					type="button"
					class="cursor-pointer rounded-md border border-neutral-700 px-3 py-1.5 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white"
					data-on:click="$creatingApp = false; $appName = ''; $appError = ''"
				>
					"Cancel"
				</button>
				<button
					type="button"
					class="cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
					data-on:click="$appError = ''; @post('/app/me/apps/token')"
					data-indicator="mintingApp"
					data-attr-disabled="$mintingApp || $appName.trim() == ''"
				>
					"Generate Token"
				</button>
			</div>
		</section>
		<section
			class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
			data-show="$replacingApp && !$appToken"
			style="display: none"
		>
			<h2 class="text-lg font-medium">"Replace app identity"</h2>
			<p class="mt-1 text-sm text-neutral-300">
				"Generates a new SpacetimeDB identity and token for this app. The name stays the same; existing account grants move to the new identity. Copy the new token once."
			</p>
			<p class="mt-4 truncate text-sm text-neutral-100">
				<span class="text-neutral-500">"Name "</span>
				<span data-text="$appName"></span>
			</p>
			<div class="mt-4 flex flex-wrap gap-2">
				<button
					type="button"
					class="cursor-pointer rounded-md border border-neutral-700 px-3 py-1.5 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white"
					data-on:click="$replacingApp = false; $appName = ''; $appError = ''"
				>
					"Cancel"
				</button>
				<button
					type="button"
					class="cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
					data-on:click="$appError = ''; @post('/app/me/apps/token')"
					data-indicator="mintingApp"
					data-attr-disabled="$mintingApp"
				>
					"Generate Token"
				</button>
			</div>
		</section>
		<section
			class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
			data-show="$appToken"
			style="display: none"
		>
			<h2 class="text-lg font-medium">"App token"</h2>
			<p class="mt-1 text-sm text-neutral-300">
				"Copy this now. The bot uses it to connect. We won't show it again."
			</p>
			<p class="mt-2 truncate text-sm text-neutral-400" data-show="$appMintName">
				<span class="text-neutral-500">"Name "</span>
				<span data-text="$appMintName"></span>
			</p>
			<p class="mt-1 truncate font-mono text-xs text-neutral-500" data-show="$appMintId">
				<span data-text="$appMintId"></span>
			</p>
			<p class="mt-4 text-xs font-medium uppercase tracking-wide text-neutral-500">"Token"</p>
			<div class="mt-1 flex items-stretch overflow-hidden rounded-lg border border-neutral-800">
				<code
					class="min-w-0 flex-1 truncate bg-neutral-900 px-3 py-2 text-sm text-anakiwa"
					data-text="$appToken"
				></code>
				<button
					type="button"
					class="shrink-0 cursor-pointer border-l border-neutral-800 px-3 text-xs text-neutral-300 hover:bg-neutral-900 hover:text-white"
					data-on:click="window.navigator.clipboard.writeText($appToken); $appCopied = true"
					data-on:click__delay.2000ms="$appCopied = false"
					data-text="$appCopied ? 'Copied' : 'Copy'"
				>
					"Copy"
				</button>
			</div>
			<button
				type="button"
				class="mt-4 cursor-pointer rounded-md bg-neutral-800 px-4 py-2 text-sm hover:bg-neutral-700"
				data-on:click="$appToken = ''; $appMintName = ''; $appMintId = ''; $appCopied = false; $appName = ''; $creatingApp = false; $replacingApp = false"
			>
				"Done"
			</button>
		</section>
	})
}

/// `POST /app/me/apps/token` — mint a host-signed identity, reserve it with a ticket, and
/// return the token in `$appToken` only.
#[route(POST "/app/me/apps/token")]
async fn token(cx: &Cx, Signals(form): Signals<TokenSignals>) -> Result<PatchSignals> {
	let _user = require_user(cx).await?;
	let name = form.app_name.trim().to_owned();
	if name.is_empty() || name.len() > 64 {
		return mint_err("Enter an app name (max 64 characters).");
	}

	let stdb = app_context::<StdbState>(cx);
	let minted = match mint_app_identity(stdb).await {
		Ok(m) => m,
		Err(e) => {
			eprintln!("app token: {e}");
			return mint_err("Couldn't mint a token from SpacetimeDB.");
		}
	};

	let conn = acquire_user_db(cx).await?;
	let ticket = if form.replacing_app {
		replace_user_app_ticket(conn.get(), name.clone(), minted.identity.clone()).await
	} else {
		create_user_app_ticket(conn.get(), name.clone(), minted.identity.clone()).await
	};
	if let Err(e) = ticket {
		return mint_err(&e);
	}

	// Connect once as the app so the ticket is fulfilled right away rather than
	// waiting on the bot's first connection (tickets expire after 15 minutes).
	// If this fails the token isn't usable, so don't hand it out.
	match stdb.pool.acquire(&minted.token).await {
		Ok(_) => stdb.pool.invalidate_token(&minted.token),
		Err(e) => {
			eprintln!("app token: fulfill connect: {e}");
			return mint_err("Couldn't register the app with SpacetimeDB. Try again.");
		}
	}

	PatchSignals::json(&MintPatch {
		app_token: minted.token,
		app_mint_name: name,
		app_mint_id: minted.identity,
		app_error: String::new(),
	})
}

#[derive(Debug, Deserialize)]
struct TokenSignals {
	#[serde(default, rename = "appName")]
	app_name: String,
	#[serde(default, rename = "replacingApp")]
	replacing_app: bool,
}

#[derive(Serialize)]
struct MintPatch {
	#[serde(rename = "appToken")]
	app_token: String,
	#[serde(rename = "appMintName")]
	app_mint_name: String,
	#[serde(rename = "appMintId")]
	app_mint_id: String,
	#[serde(rename = "appError")]
	app_error: String,
}

#[derive(Serialize)]
struct MintErrorPatch {
	#[serde(rename = "appError")]
	app_error: String,
}

fn mint_err(error: &str) -> Result<PatchSignals> {
	PatchSignals::json(&MintErrorPatch {
		app_error: error.to_owned(),
	})
}

fn js_single(s: &str) -> String {
	let mut out = String::from("'");
	for c in s.chars() {
		match c {
			'\\' => out.push_str("\\\\"),
			'\'' => out.push_str("\\'"),
			'\n' => out.push_str("\\n"),
			c => out.push(c),
		}
	}
	out.push('\'');
	out
}

fn escape_html(s: &str) -> String {
	let mut out = String::with_capacity(s.len());
	for c in s.chars() {
		match c {
			'&' => out.push_str("&amp;"),
			'<' => out.push_str("&lt;"),
			'>' => out.push_str("&gt;"),
			'"' => out.push_str("&quot;"),
			'\'' => out.push_str("&#39;"),
			c => out.push(c),
		}
	}
	out
}
