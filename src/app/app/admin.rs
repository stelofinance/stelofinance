//! `GET /app/admin` — platform-admin reducer forms. Start: `create_ledger`.

use crate::auth::require_user;
use crate::auth::user::AppUser;
use crate::module_bindings::LedgerKind;
use crate::stdb::{acquire_user_db, create_user_ledger};
use serde::{Deserialize, Serialize};
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchSignals, Signals},
	router::{error::not_found, page, route},
	view::view,
};

async fn require_admin(cx: &Cx) -> Result<&AppUser> {
	let user = require_user(cx).await?;
	if !user.is_admin {
		return Err(not_found().into());
	}
	Ok(user)
}

/// `GET /app/admin` — 404 unless `User.is_admin`.
#[page]
async fn page(cx: &Cx) -> Result {
	let _user = require_admin(cx).await?;
	view! {
		<main
			id="page-content"
			class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
			data-signals="{ledgerName:'',ledgerScale:'0',ledgerKind:'Physical',ledgerError:'',ledgerOk:''}"
		>
			<h1 class="mb-2 text-2xl font-medium md:text-3xl">"Admin"</h1>
			<p class="mb-6 text-sm text-neutral-400">"Forms that call admin-only reducers."</p>

			<section class="rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5">
				<h2 class="text-lg font-medium">"Create ledger"</h2>
				<p class="mt-1 text-sm text-neutral-300">
					"Adds a public asset to the catalog ("
					<code class="text-neutral-200">"create_ledger"</code>
					")."
				</p>

				<label class="mt-4 block">
					<span class="text-sm text-neutral-300">"Name"</span>
					<input
						type="text"
						class="mt-1 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
						data-bind="ledgerName"
						placeholder="Hexcoin"
						autocomplete="off"
					>
				</label>

				<label class="mt-4 block">
					<span class="text-sm text-neutral-300">"Scale "</span>
					<span class="text-sm text-neutral-400">"(decimal places)"</span>
					<input
						type="text"
						inputmode="numeric"
						class="mt-1 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
						data-bind="ledgerScale"
						placeholder="0"
						autocomplete="off"
					>
				</label>

				<label class="mt-4 block">
					<span class="text-sm text-neutral-300">"Kind"</span>
					<select
						class="mt-1 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm"
						data-bind="ledgerKind"
					>
						<option value="Physical">"In-game item"</option>
						<option value="Digital">"Digital"</option>
						<option value="Derivation">"Derivation"</option>
					</select>
				</label>

				<button
					type="button"
					class="mt-5 w-full cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
					data-on:click="@post('/app/admin/ledgers')"
					data-indicator="creatingLedger"
					data-attr:disabled="$creatingLedger || $ledgerName.trim() == ''"
				>
					"Create ledger"
				</button>
				<p
					class="mt-3 text-sm text-red-400"
					data-show="$ledgerError"
					data-text="$ledgerError"
				></p>
				<p
					class="mt-3 text-sm text-anakiwa"
					data-show="$ledgerOk"
					data-text="$ledgerOk"
				></p>
			</section>
		</main>
	}
}

#[derive(Debug, Deserialize)]
struct LedgerSignals {
	#[serde(default, rename = "ledgerName")]
	name: String,
	#[serde(default, rename = "ledgerScale")]
	scale: String,
	#[serde(default, rename = "ledgerKind")]
	kind: String,
}

#[derive(Serialize)]
struct LedgerPatch {
	#[serde(rename = "ledgerName")]
	name: String,
	#[serde(rename = "ledgerScale")]
	scale: String,
	#[serde(rename = "ledgerKind")]
	kind: String,
	#[serde(rename = "ledgerError")]
	error: String,
	#[serde(rename = "ledgerOk")]
	ok: String,
}

/// `POST /app/admin/ledgers` — `create_ledger`.
#[route(POST "/app/admin/ledgers")]
async fn ledgers(cx: &Cx, Signals(form): Signals<LedgerSignals>) -> Result<PatchSignals> {
	let _user = require_admin(cx).await?;
	let name = form.name.trim().to_owned();
	if name.is_empty() {
		return ledger_patch(&form, "Ledger name required.", "");
	}
	let Ok(scale) = form.scale.trim().parse::<u8>() else {
		return ledger_patch(&form, "Scale must be 0–255.", "");
	};
	let kind = match form.kind.trim() {
		"Digital" => LedgerKind::Digital,
		"Derivation" => LedgerKind::Derivation,
		"Physical" => LedgerKind::Physical,
		_ => return ledger_patch(&form, "Pick a ledger kind.", ""),
	};

	let conn = acquire_user_db(cx).await?;
	match create_user_ledger(conn.get(), name.clone(), scale, kind).await {
		Ok(()) => ledger_patch(
			&LedgerSignals {
				name: String::new(),
				scale: form.scale.clone(),
				kind: form.kind.clone(),
			},
			"",
			&format!("Created {name}."),
		),
		Err(e) => ledger_patch(&form, &e, ""),
	}
}

fn ledger_patch(form: &LedgerSignals, error: &str, ok: &str) -> Result<PatchSignals> {
	PatchSignals::json(&LedgerPatch {
		name: form.name.clone(),
		scale: form.scale.clone(),
		kind: form.kind.clone(),
		error: error.to_owned(),
		ok: ok.to_owned(),
	})
}
