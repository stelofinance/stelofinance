//! App list HTML, mint sheet, and `GET /app/me/apps/mint`.

use crate::auth::cookies::{COOKIE_STAUTH_MINT, clear_cookie, get_cookie};
use crate::auth::require_user;
use crate::auth::spacetimeauth::SpacetimeAuthState;
use crate::module_bindings::AppTicketPurpose;
use crate::stdb::apps::MyAppsData;
use crate::stdb::{format_rel_time, unix_now_micros};
use serde::Serialize;
use topcoat::{
	Result,
	context::{Cx, app_context},
	datastar::PatchSignals,
	router::route,
	view::{Unescaped, component, view},
};

#[component]
pub async fn my_apps_list(data: MyAppsData, mint_enabled: bool) -> Result {
	view! {
		(Unescaped::new_unchecked(my_apps_html(&data, mint_enabled)))
	}
}

pub fn my_apps_html(data: &MyAppsData, mint_enabled: bool) -> String {
	let mut out = String::from(r#"<div id="me-apps" class="flex flex-col">"#);
	out.push_str(r#"<section class="mt-8">"#);
	out.push_str(r#"<div class="flex flex-wrap items-center justify-between gap-3">"#);
	out.push_str(
		r#"<h2 class="text-sm font-medium uppercase tracking-wide text-neutral-400">Apps</h2>"#,
	);
	if mint_enabled {
		out.push_str(
			r#"<button type="button" class="cursor-pointer rounded-md bg-anakiwa-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50" data-attr:disabled="$creatingApp || $replacingApp || $appIdToken != ''" data-on:click="$creatingApp = true; $replacingApp = false; $appError = ''; $appName = ''">New app</button>"#,
		);
	}
	out.push_str("</div>");
	out.push_str(
		r#"<p class="mt-2 text-sm text-neutral-400">Bots you own. Copy tokens once, then grant a role on an account under Permissions.</p>"#,
	);
	if !mint_enabled {
		out.push_str(
			r#"<p class="mt-3 text-sm text-neutral-300">SpacetimeAuth isn't loaded. Set <code class="text-anakiwa">SPACETIMEAUTH_CLIENT_ID</code>, <code class="text-anakiwa">CLIENT_SECRET</code>, and <code class="text-anakiwa">REDIRECT_URL</code> in <code class="text-anakiwa">.env</code>, then restart the edge. Log line: <code class="text-anakiwa">spacetimeauth: OIDC client ready</code>.</p>"#,
		);
	} else if data.apps.is_empty() && data.tickets.is_empty() {
		out.push_str(
			r#"<p class="mt-3 text-sm text-neutral-400">No apps yet. Use <span class="text-neutral-200">New app</span> to mint tokens.</p>"#,
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
			out.push_str(&format!(
				r#"<div class="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2.5"><div class="min-w-0"><p class="truncate text-sm text-neutral-100">{name}</p><p class="mt-0.5 truncate font-mono text-xs text-neutral-500">{hex}</p></div>"#
			));
			if mint_enabled {
				let name_js = js_single(&a.name);
				out.push_str(&format!(
					r#"<button type="button" class="cursor-pointer text-sm text-neutral-300 hover:text-white disabled:cursor-not-allowed disabled:opacity-50" data-attr:disabled="$creatingApp || $replacingApp || $appIdToken != ''" data-on:click="$replacingApp = true; $creatingApp = false; $appError = ''; $appName = {name_js}">Replace</button>"#
				));
			}
			out.push_str("</div>");
		}
		out.push_str("</div>");
	}
	out.push_str("</section></div>");
	out
}

/// New-app form + one-time token sheet (outside live remorph).
#[component]
pub async fn app_forms(mint_enabled: bool) -> Result {
	let go_create = "window.location.href = '/auth/spacetimeauth/login?redirect=/app/me&purpose=create&name=' + encodeURIComponent($appName.trim())".to_owned();
	let go_replace = "window.location.href = '/auth/spacetimeauth/login?redirect=/app/me&purpose=replace&name=' + encodeURIComponent($appName.trim())".to_owned();
	view! {
		if mint_enabled {
			<section
				class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
				data-show="$creatingApp && !$appIdToken"
				style="display: none"
			>
				<h2 class="text-lg font-medium">"New app"</h2>
				<p class="mt-1 text-sm text-neutral-300">
					"Signs in with SpacetimeAuth (anonymous) to mint bot tokens. Copy them once. Then grant the app a role on an account."
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
						data-on:click=(go_create)
						data-attr-disabled="$appName.trim() == ''"
					>
						"Continue with SpacetimeAuth"
					</button>
				</div>
			</section>
			<section
				class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
				data-show="$replacingApp && !$appIdToken"
				style="display: none"
			>
				<h2 class="text-lg font-medium">"Replace app identity"</h2>
				<p class="mt-1 text-sm text-neutral-300">
					"Mints a new SpacetimeAuth identity and tokens for this app. The name stays the same; existing account grants move to the new identity. Copy the new tokens once."
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
						class="cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
						data-on:click=(go_replace)
					>
						"Continue with SpacetimeAuth"
					</button>
				</div>
			</section>
		}
		<section
			class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
			data-show="$appIdToken"
			style="display: none"
		>
			<h2 class="text-lg font-medium">"App tokens"</h2>
			<p class="mt-1 text-sm text-neutral-300">
				"Copy these now. The bot uses them to connect. We won't show them again."
			</p>
			<p class="mt-2 truncate text-sm text-neutral-400" data-show="$appMintName">
				<span class="text-neutral-500">"Name "</span>
				<span data-text="$appMintName"></span>
			</p>
			<p class="mt-1 truncate font-mono text-xs text-neutral-500" data-show="$appMintId">
				<span data-text="$appMintId"></span>
			</p>
			<p class="mt-4 text-xs font-medium uppercase tracking-wide text-neutral-500">
				"ID token"
			</p>
			<div class="mt-1 flex items-stretch overflow-hidden rounded-lg border border-neutral-800">
				<code
					class="min-w-0 flex-1 truncate bg-neutral-900 px-3 py-2 text-sm text-anakiwa"
					data-text="$appIdToken"
				></code>
				<button
					type="button"
					class="shrink-0 cursor-pointer border-l border-neutral-800 px-3 text-xs text-neutral-300 hover:bg-neutral-900 hover:text-white"
					data-on:click="window.navigator.clipboard.writeText($appIdToken); $appCopied = 'id'"
					data-on:click__delay.2000ms="$appCopied = ''"
					data-text="$appCopied == 'id' ? 'Copied' : 'Copy'"
				>
					"Copy"
				</button>
			</div>
			<p
				class="mt-4 text-xs font-medium uppercase tracking-wide text-neutral-500"
				data-show="$appRefreshToken"
			>
				"Refresh token"
			</p>
			<div
				class="mt-1 flex items-stretch overflow-hidden rounded-lg border border-neutral-800"
				data-show="$appRefreshToken"
				style="display: none"
			>
				<code
					class="min-w-0 flex-1 truncate bg-neutral-900 px-3 py-2 text-sm text-anakiwa"
					data-text="$appRefreshToken"
				></code>
				<button
					type="button"
					class="shrink-0 cursor-pointer border-l border-neutral-800 px-3 text-xs text-neutral-300 hover:bg-neutral-900 hover:text-white"
					data-on:click="window.navigator.clipboard.writeText($appRefreshToken); $appCopied = 'refresh'"
					data-on:click__delay.2000ms="$appCopied = ''"
					data-text="$appCopied == 'refresh' ? 'Copied' : 'Copy'"
				>
					"Copy"
				</button>
			</div>
			<button
				type="button"
				class="mt-4 cursor-pointer rounded-md bg-neutral-800 px-4 py-2 text-sm hover:bg-neutral-700"
				data-on:click="$appIdToken = ''; $appRefreshToken = ''; $appMintName = ''; $appMintId = ''; $appCopied = ''; $appName = ''; $creatingApp = false; $replacingApp = false"
			>
				"Done"
			</button>
		</section>
	}
}

#[route(GET "/app/me/apps/mint")]
async fn mint(cx: &Cx) -> Result<PatchSignals> {
	let user = require_user(cx).await?;
	let stauth = app_context::<SpacetimeAuthState>(cx);
	let Some(key) = get_cookie(cx, COOKIE_STAUTH_MINT) else {
		return empty_mint();
	};
	clear_cookie(cx, COOKIE_STAUTH_MINT);
	let Some(flash) = stauth.mints.take(&key) else {
		return empty_mint();
	};
	if flash.user != user.identity {
		return empty_mint();
	}
	PatchSignals::json(&MintPatch {
		app_id_token: flash.id_token,
		app_refresh_token: flash.refresh_token.unwrap_or_default(),
		app_mint_name: flash.name,
		app_mint_id: flash.identity_hex.unwrap_or_default(),
		app_error: String::new(),
	})
}

#[derive(Serialize)]
struct MintPatch {
	#[serde(rename = "appIdToken")]
	app_id_token: String,
	#[serde(rename = "appRefreshToken")]
	app_refresh_token: String,
	#[serde(rename = "appMintName")]
	app_mint_name: String,
	#[serde(rename = "appMintId")]
	app_mint_id: String,
	#[serde(rename = "appError")]
	app_error: String,
}

fn empty_mint() -> Result<PatchSignals> {
	PatchSignals::json(&MintPatch {
		app_id_token: String::new(),
		app_refresh_token: String::new(),
		app_mint_name: String::new(),
		app_mint_id: String::new(),
		app_error: String::new(),
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
