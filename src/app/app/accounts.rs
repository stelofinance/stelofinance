//! `GET/POST /app/accounts` — portfolio + create. Live list: the `list` shard.

mod account_id;
mod list;

use crate::auth::require_user;
use crate::domain::{has_primary_on_ledger, parse_kind};
use crate::module_bindings::{AccountKind, Ledger};
use crate::stdb::{acquire_user_db, create_user_account, fetch_accounts_page};
use crate::ui::text::ledger_kind_label;
use list::accounts_list;
use serde::{Deserialize, Serialize};
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchSignals, Signals},
	metadata::Metadata,
	router::{error::internal_server_error, page, route},
	view::{View, ViewExt, component, view},
};

/// `GET /app/accounts` — SSR current `my_accounts` + create sheet.
#[page(with(Metadata::new().title("Accounts · Stelo Finance")))]
async fn page(cx: &Cx) -> Result<impl View> {
	let user = require_user(cx).await?;
	let conn = acquire_user_db(cx).await?;
	let data = fetch_accounts_page(conn.get())
		.await
		.map_err(internal_server_error)?;

	let initial_ledger_id = if data.ledgers.len() == 1 {
		data.ledgers[0].id.to_string()
	} else {
		String::new()
	};
	let primary_ids = encode_primary_ids(
		data.accounts
			.iter()
			.filter(|a| a.is_primary)
			.map(|a| a.ledger_id),
	);
	let signals = format!(
		"{{creatingAcc:false,ledgerId:'{initial_ledger_id}',accountKind:'debit',address:'',label:'',createError:'',copiedId:0,primaryIds:'{primary_ids}'}}"
	);

	Ok(view! {
    <main
        id="page-content"
        class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
        data-signals=(signals)
    >
        page_header(
            can_create: !data.ledgers.is_empty(),
            initial_ledger_id: initial_ledger_id.clone()
        )
        create_form(
            ledgers: data.ledgers,
            username: user.bitcraft_username.clone(),
            is_admin: user.is_admin
        )
        accounts_list()
    </main>
})
}

/// `POST /app/accounts` — command only. The live `accounts_list` shard shows the new row.
#[route(POST)]
async fn create(cx: &Cx, Signals(form): Signals<CreateAccountSignals>) -> Result<PatchSignals> {
	let user = require_user(cx).await?;
	let ledger_id = form.ledger_id.trim();
	if ledger_id.is_empty() {
		return create_signals(true, "Choose an asset.", &form, None, None);
	}
	let Ok(ledger_id) = ledger_id.parse::<u64>() else {
		return create_signals(true, "Choose an asset.", &form, None, None);
	};

	let kind = match parse_kind(&form.account_kind) {
		Some(AccountKind::Credit) if user.is_admin => AccountKind::Credit,
		Some(AccountKind::Credit) => {
			return create_signals(
				true,
				"Only platform admins can create credit accounts.",
				&form,
				None,
				None,
			);
		}
		_ => AccountKind::Debit,
	};

	let address = form.address.trim();
	let address = if address.is_empty() {
		None
	} else if user.is_admin {
		Some(address.to_owned())
	} else {
		return create_signals(true, "Custom address is admin-only.", &form, None, None);
	};

	let label = form.label.trim();
	let label = if label.is_empty() {
		None
	} else {
		Some(label.to_owned())
	};

	let conn = acquire_user_db(cx).await?;
	let snapshot = match fetch_accounts_page(conn.get()).await {
		Ok(p) => p,
		Err(e) => return create_signals(true, e.user_message(), &form, None, None),
	};
	let is_primary =
		matches!(kind, AccountKind::Debit) && !has_primary_on_ledger(&snapshot.accounts, ledger_id);

	match create_user_account(conn.get(), ledger_id, kind, address, label, is_primary).await {
		Ok(()) => {
			let mut ids: Vec<u64> = snapshot
				.accounts
				.iter()
				.filter(|a| a.is_primary)
				.map(|a| a.ledger_id)
				.collect();
			if is_primary && !ids.contains(&ledger_id) {
				ids.push(ledger_id);
			}
			create_signals(
				false,
				"",
				&form,
				Some("debit"),
				Some(encode_primary_ids(ids)),
			)
		}
		Err(e) => create_signals(true, e.user_message(), &form, None, None),
	}
}

#[derive(Debug, Deserialize)]
struct CreateAccountSignals {
	#[serde(default, rename = "ledgerId")]
	ledger_id: String,
	#[serde(default, rename = "accountKind")]
	account_kind: String,
	#[serde(default)]
	address: String,
	#[serde(default)]
	label: String,
}

#[derive(Serialize)]
struct CreateAccountPatch {
	#[serde(rename = "creatingAcc")]
	creating_acc: bool,
	#[serde(rename = "createError")]
	create_error: String,
	#[serde(rename = "ledgerId")]
	ledger_id: String,
	#[serde(rename = "accountKind")]
	account_kind: String,
	address: String,
	label: String,
	#[serde(rename = "primaryIds", skip_serializing_if = "Option::is_none")]
	primary_ids: Option<String>,
}

pub(super) fn encode_primary_ids(ids: impl IntoIterator<Item = u64>) -> String {
	let mut out = String::new();
	for id in ids {
		if out.is_empty() {
			out.push('|');
		}
		out.push_str(&id.to_string());
		out.push('|');
	}
	out
}

fn create_signals(
	creating_acc: bool,
	error: &str,
	form: &CreateAccountSignals,
	reset_kind: Option<&str>,
	primary_ids: Option<String>,
) -> Result<PatchSignals> {
	let success = !creating_acc && error.is_empty();
	PatchSignals::json(&CreateAccountPatch {
		creating_acc,
		create_error: error.to_owned(),
		ledger_id: form.ledger_id.clone(),
		account_kind: reset_kind.unwrap_or(form.account_kind.as_str()).to_owned(),
		address: if success {
			String::new()
		} else {
			form.address.clone()
		},
		label: if success {
			String::new()
		} else {
			form.label.clone()
		},
		primary_ids,
	})
}

#[component]
async fn page_header(cx: &Cx, can_create: bool, initial_ledger_id: String) -> Result<impl View> {
	let open = format!(
		"$creatingAcc = true; $createError = ''; $ledgerId = '{initial_ledger_id}'; $accountKind = 'debit'; $address = ''; $label = ''"
	);

	Ok(view! {
    <div class="mb-6 flex items-center justify-between gap-3">
        <h1 class="text-2xl font-medium md:text-3xl">"Accounts"</h1>
        if can_create {
            <button
                type="button"
                class="rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
                data-on:click=(open)
                data-show="!$creatingAcc"
            >
                "New account"
            </button>
        }
    </div>
})
}

#[component]
async fn create_form(
	cx: &Cx,
	ledgers: Vec<Ledger>,
	username: String,
	is_admin: bool,
) -> Result<impl View> {
	if ledgers.is_empty() {
		return Ok(view! {
    <p class="mt-2 text-sm text-neutral-400">"No assets are on the platform yet."</p>
}
		.boxed());
	}

	Ok(view! {
    <div
        data-show="$creatingAcc"
        class="mb-6 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
        style="display: none"
    >
        <h2 class="text-lg font-medium">"New account"</h2>
        <p class="mt-1 text-sm text-neutral-300">
            "Choose the asset this account will hold."
        </p>

        <div class="mt-4 grid grid-cols-1 gap-2 sm:grid-cols-2">
            for ledger in &ledgers {
                ledger_choice(ledger: ledger)
            }
        </div>

        for ledger in &ledgers {
            create_helper(ledger: ledger, username: username.clone())
        }

        <p
            class="mt-3 text-sm text-neutral-300"
            data-show="$accountKind == 'credit' && $ledgerId != ''"
        >
            "Credit accounts sit on the issuer side of this asset. They cannot be primary."
        </p>

        <label class="mt-4 block">
            <span class="text-sm text-neutral-300">"Label "</span>
            <span class="text-sm text-neutral-400">"(optional)"</span>
            <input
                type="text"
                class="mt-1 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
                data-bind="label"
                maxlength="32"
                placeholder="e.g. Guild treasury"
                autocomplete="off"
            >
        </label>

        if is_admin {
            admin_advanced()
        }

        <p
            class="mt-3 text-sm text-red-400"
            data-show="$createError"
            data-text="$createError"
        ></p>

        <div class="mt-5 flex justify-end gap-2">
            <button
                type="button"
                class="rounded-md border border-neutral-700 px-4 py-2 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white"
                data-on:click="$creatingAcc = false; $createError = ''"
            >
                "Cancel"
            </button>
            <button
                type="button"
                class="rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
                data-on:click="@post('/app/accounts')"
                data-indicator="creating"
                data-attr:disabled="$creating || !$ledgerId"
            >
                "Create"
            </button>
        </div>
    </div>
}.boxed())
}

#[component]
async fn ledger_choice(cx: &Cx, ledger: &Ledger) -> Result<impl View> {
	let id = ledger.id.to_string();
	let click = format!("$ledgerId = '{id}'; $createError = ''");
	let is_on = format!("$ledgerId == '{id}'");
	let is_off = format!("$ledgerId != '{id}'");
	// Object form: `view!` rejects `data-class:bg-…` and the hyphen form is ignored.
	let cls = format!(
		"{{'border-anakiwa': {is_on}, 'bg-anakiwa-900': {is_on}, 'border-neutral-800': {is_off}, 'bg-neutral-900': {is_off}}}"
	);
	let kind = ledger_kind_label(ledger.kind);

	Ok(view! {
    <button
        type="button"
        class="flex w-full cursor-pointer items-start justify-between gap-3 rounded-lg border border-neutral-800 bg-neutral-900 p-4 text-left transition-colors hover:border-neutral-600"
        data-class=(cls)
        data-on:click=(click)
    >
        <div class="min-w-0">
            <p class="font-medium">(ledger.name.clone())</p>
            <p class="mt-0.5 text-xs text-neutral-400">(kind)</p>
        </div>
        <span
            class="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border-2 border-neutral-600"
            data-show=(is_off)
        ></span>
        <span
            class="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border-2 border-anakiwa bg-anakiwa text-xs font-bold text-neutral-900"
            data-show=(is_on)
            style="display: none"
        >
            "✓"
        </span>
    </button>
})
}

#[component]
async fn create_helper(cx: &Cx, ledger: &Ledger, username: String) -> Result<impl View> {
	let id = ledger.id;
	let show_first = format!(
		"$ledgerId == '{id}' && $accountKind != 'credit' && !$primaryIds.includes('|{id}|')"
	);
	let show_existing = format!(
		"$ledgerId == '{id}' && $accountKind != 'credit' && $primaryIds.includes('|{id}|')"
	);
	let first = format!(
		"This will be your primary {} account. People can send to @{username}.",
		ledger.name
	);
	let existing = format!("Friends sending to @{username} still go to your primary.");

	Ok(view! {
    <p class="mt-3 text-sm text-neutral-300" data-show=(show_first)>(first)</p>
    <p class="mt-3 text-sm text-neutral-300" data-show=(show_existing)>(existing)</p>
})
}

#[component]
async fn admin_advanced(cx: &Cx) -> Result<impl View> {
	Ok(view! {
    <details class="mt-4 rounded-md border border-neutral-800">
        <summary
            class="cursor-pointer px-3 py-2 text-sm text-neutral-300 hover:text-white"
        >
            "Advanced"
        </summary>
        <div class="flex flex-col gap-3 border-t border-neutral-800 px-3 py-3">
            <label class="block">
                <span class="text-sm text-neutral-300">"Kind"</span>
                <select
                    class="mt-1 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm"
                    data-bind="accountKind"
                >
                    <option value="debit">"Debit"</option>
                    <option value="credit">"Credit (issuer)"</option>
                </select>
            </label>
            <label class="block">
                <span class="text-sm text-neutral-300">"Custom address "</span>
                <span class="text-sm text-neutral-400">"(optional)"</span>
                <input
                    type="text"
                    class="mt-1 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm uppercase placeholder:text-neutral-400"
                    data-bind="address"
                    maxlength="16"
                    placeholder="Leave blank to generate"
                    autocomplete="off"
                    spellcheck="false"
                >
            </label>
        </div>
    </details>
})
}
