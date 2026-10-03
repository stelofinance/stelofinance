//! Create / revoke API tokens on an account (Admin+).

use super::AccountId;
use crate::auth::require_user;
use crate::stdb::account::{create_account_api_token, revoke_account_api_tokens};
use crate::stdb::acquire_user_db;
use serde::{Deserialize, Serialize};
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchSignals, Signals},
	router::{path_param, route},
	view::{component, view},
};

/// Create + one-time secret sheet. Lives outside `#account-home` so remorphs keep it.
#[component]
pub async fn token_forms(account_id: u64) -> Result {
	let create_url = format!("@post('/app/accounts/{account_id}/tokens')");
	view! {
		<section
			class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
			data-show="$creatingToken && !$newToken"
			style="display: none"
		>
			<h2 class="text-lg font-medium">"New token"</h2>
			<p class="mt-1 text-sm text-neutral-300">
				"The secret is shown once. Copy it then, it won't be shown again."
			</p>
			<input
				type="text"
				class="mt-4 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm placeholder:text-neutral-400"
				data-bind="tokenLabel"
				maxlength="32"
				placeholder="Label (optional)"
				autocomplete="off"
			>
			<p
				class="mt-3 text-sm text-red-400"
				data-show="$tokenError"
				data-text="$tokenError"
			></p>
			<div class="mt-4 flex flex-wrap gap-2">
				<button
					type="button"
					class="cursor-pointer rounded-md border border-neutral-700 px-3 py-1.5 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white"
					data-on:click="$creatingToken = false; $tokenLabel = ''; $tokenError = ''"
				>
					"Cancel"
				</button>
				<button
					type="button"
					class="cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
					data-on:click=(create_url)
					data-indicator="creatingTokenBusy"
					data-attr-disabled="$creatingTokenBusy"
				>
					"Create"
				</button>
			</div>
		</section>

		<section
			class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
			data-show="$newToken"
			style="display: none"
		>
			<h2 class="text-lg font-medium">"Token created"</h2>
			<p class="mt-1 text-sm text-neutral-300">
				"Copy it now, this is the only time you’ll see it."
			</p>
			<div class="mt-4 flex items-stretch overflow-hidden rounded-lg border border-neutral-800">
				<code
					class="min-w-0 flex-1 truncate bg-neutral-900 px-3 py-2 text-sm text-anakiwa"
					data-text="$newToken"
				></code>
				<button
					type="button"
					class="shrink-0 cursor-pointer border-l border-neutral-800 px-3 text-xs text-neutral-300 hover:bg-neutral-900 hover:text-white"
					data-on:click="window.navigator.clipboard.writeText($newToken); $tokenCopied = true"
					data-on:click__delay.2000ms="$tokenCopied = false"
					data-text="$tokenCopied ? 'Copied' : 'Copy'"
				>
					"Copy"
				</button>
			</div>
			<button
				type="button"
				class="mt-4 cursor-pointer rounded-md bg-neutral-800 px-4 py-2 text-sm hover:bg-neutral-700"
				data-on:click="$newToken = ''; $creatingToken = false; $tokenLabel = ''; $tokenCopied = false; $tokenError = ''"
			>
				"Done"
			</button>
		</section>
	}
}

/// `POST /app/accounts/{account_id}/tokens` — mint; secret in `$newToken` only.
#[route(POST)]
async fn create_token(cx: &Cx, Signals(form): Signals<CreateTokenSignals>) -> Result<PatchSignals> {
	let _user = require_user(cx).await?;
	let account_id = *path_param::<AccountId>(cx)?;
	let label = form.token_label.trim().to_owned();
	let conn = acquire_user_db(cx).await?;
	match create_account_api_token(conn.get(), account_id, label).await {
		Ok(secret) => PatchSignals::json(&TokenCreatedPatch {
			token_error: String::new(),
			new_token: secret,
			creating_token: false,
			token_label: String::new(),
		}),
		Err(e) => token_err(&e),
	}
}

/// `POST /app/accounts/{account_id}/tokens/revoke`
#[route(POST "/app/accounts/{account_id}/tokens/revoke")]
async fn revoke(cx: &Cx, Signals(form): Signals<RevokeTokenSignals>) -> Result<PatchSignals> {
	let _user = require_user(cx).await?;
	let account_id = *path_param::<AccountId>(cx)?;
	if form.revoke_token_id == 0 {
		return token_err("Pick a token to revoke.");
	}
	let conn = acquire_user_db(cx).await?;
	match revoke_account_api_tokens(conn.get(), account_id, vec![form.revoke_token_id]).await {
		Ok(()) => PatchSignals::json(&TokenRevokedPatch {
			token_error: String::new(),
			revoke_token_id: 0,
		}),
		Err(e) => token_err(&e),
	}
}

#[derive(Debug, Deserialize)]
struct CreateTokenSignals {
	#[serde(default, rename = "tokenLabel")]
	token_label: String,
}

#[derive(Debug, Deserialize)]
struct RevokeTokenSignals {
	#[serde(default, rename = "revokeTokenId")]
	revoke_token_id: u64,
}

#[derive(Serialize)]
struct TokenCreatedPatch {
	#[serde(rename = "tokenError")]
	token_error: String,
	#[serde(rename = "newToken")]
	new_token: String,
	#[serde(rename = "creatingToken")]
	creating_token: bool,
	#[serde(rename = "tokenLabel")]
	token_label: String,
}

#[derive(Serialize)]
struct TokenRevokedPatch {
	#[serde(rename = "tokenError")]
	token_error: String,
	#[serde(rename = "revokeTokenId")]
	revoke_token_id: u64,
}

#[derive(Serialize)]
struct TokenErrorPatch {
	#[serde(rename = "tokenError")]
	token_error: String,
}

fn token_err(error: &str) -> Result<PatchSignals> {
	PatchSignals::json(&TokenErrorPatch {
		token_error: error.to_owned(),
	})
}
