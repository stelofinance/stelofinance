//! `GET /app/accounts/{account_id}` — account home.
//!
//! The page owns the signals. The live shard (`live.rs`) renders home,
//! people and tokens; the forms around it are static and own their input
//! state. Every action is a `#[procedure]` taking `account_id`.

mod live;
mod permission;
mod request;
mod tokens;
mod webhook;

use crate::auth::require_user;
use crate::domain::role_rank;
use crate::module_bindings::{AccountKind, Role};
use crate::stdb::account::{fetch_account_home, set_label};
use crate::stdb::acquire_user_db;
use live::account_live;
use permission::permission_panel;
use request::request_form;
use tokens::token_forms;
use topcoat::{
	Result,
	context::Cx,
	metadata::Metadata,
	router::{
		error::{internal_server_error, not_found},
		module_param, page,
	},
	runtime::{Event, Signal, procedure, signal},
	view::{View, component, view},
};
use webhook::webhook_form;

// `module_param!` makes this module's URL segment `{account_id}` and declares
// the `account_id(cx)` accessor (also used by the absolute `{account_id}` routes).
module_param!(pub account_id: u64 else not_found);

#[page(with(Metadata::new().title("Account · Stelo Finance")))]
async fn show(cx: &Cx) -> Result<impl View> {
	require_user(cx).await?;
	let account_id = *account_id(cx)?;
	let conn = acquire_user_db(cx).await?;
	let data = fetch_account_home(conn.get(), account_id)
		.await
		.map_err(internal_server_error)?;
	let Some(acc) = &data.account else {
		return Err(not_found().into());
	};

	let admin_plus = role_rank(acc.role) >= role_rank(Role::Admin);
	let debit = matches!(acc.kind, AccountKind::Debit);
	let ledger_name = acc.ledger_name.clone();
	let label = acc.label.clone().unwrap_or_default();
	let webhook = acc.webhook.clone().unwrap_or_default();

	Ok(view! {
    <main
        id="page-content"
        class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
    >
        <a href="/app/accounts" class="text-sm text-neutral-300 hover:text-white">
            "← Accounts"
        </a>
        account_view(
            account_id: account_id,
            ledger_name: ledger_name,
            debit: debit,
            admin_plus: admin_plus,
            label: label,
            webhook: webhook
        )
    </main>
})
}

/// Owns the signals shared between the live region and the forms. Children
/// of `main` are placed with `order-*` because the live shard renders home,
/// people and tokens as one block (1, 3, 5) with the static forms between
/// them (2, 4, 6).
#[component]
async fn account_view(
	cx: &Cx,
	account_id: u64,
	ledger_name: String,
	debit: bool,
	admin_plus: bool,
	label: String,
	webhook: String,
) -> Result<impl View> {
	let account_error = signal(cx, String::new);
	let saving_primary = signal(cx, || false);
	let creating_token = signal(cx, || false);
	let new_token = signal(cx, String::new);
	let has_token = signal(cx, || false);
	let token_label = signal(cx, String::new);
	let token_error = signal(cx, String::new);
	let revoke_token_id = signal(cx, || 0u64);

	Ok(view! {
    account_live(
        account_id: account_id,
        account_error: account_error.clone(),
        saving_primary: saving_primary.clone(),
        creating_token: creating_token.clone(),
        has_token: has_token.clone(),
        token_label: token_label.clone(),
        token_error: token_error.clone(),
        revoke_token_id: revoke_token_id.clone()
    )
    <div class="order-2 flex flex-col">
        request_form(account_id: account_id, ledger_name: ledger_name, debit: debit)
    </div>
    if admin_plus {
        <div class="order-4 flex flex-col">
            permission_panel(account_id: account_id)
            admin_forms(
                account_id: account_id,
                initial_label: label,
                error: account_error.clone()
            )
        </div>
        <div class="order-6 flex flex-col">
            token_forms(
                account_id: account_id,
                creating_token: creating_token.clone(),
                new_token: new_token.clone(),
                has_token: has_token.clone(),
                token_label: token_label.clone(),
                token_error: token_error.clone()
            )
            webhook_form(account_id: account_id, initial_url: webhook)
        </div>
    }
    <p
        class="order-7 mt-4 text-sm text-red-400"
        :hidden=$(account_error.get().is_empty())
    >
        $(account_error.get())
    </p>
})
}

#[component]
async fn admin_forms(
	cx: &Cx,
	account_id: u64,
	initial_label: String,
	error: Signal<String>,
) -> Result<impl View> {
	let label = signal(cx, || initial_label.clone());
	let busy = signal(cx, || false);
	Ok(view! {
    <section
        class="mt-8 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
    >
        <h2 class="text-sm font-medium uppercase tracking-wide text-neutral-400">
            "Label"
        </h2>
        <p class="mt-1 text-sm text-neutral-400">
            "A nickname only people on this account can see."
        </p>
        <div class="mt-3 flex flex-col gap-2 sm:flex-row">
            <input
                type="text"
                class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm placeholder:text-neutral-400"
                :value=$(label.get())
                @input=$(|e: Event| label.set(e.target.value))
                maxlength="32"
                placeholder="e.g. Guild treasury"
                autocomplete="off"
            >
            <button
                type="button"
                class="cursor-pointer rounded-md bg-neutral-800 px-4 py-2 text-sm hover:bg-neutral-700 disabled:cursor-not-allowed disabled:opacity-50"
                :disabled=$(busy.get())
                @click=$(async |_e| {
                    busy.set(true);
                    let result = save_label(account_id, label.get()).await;
                    busy.set(false);
                    if result.is_ok() {
                        error.set("".to_owned());
                    } else {
                        error.set(result.unwrap_err());
                    }
                })
            >
                "Save"
            </button>
        </div>
    </section>
})
}

/// Set or clear (blank) the account nickname. The module enforces admin+.
#[procedure("/app/accounts/label")]
async fn save_label(
	cx: &Cx,
	account_id: u64,
	label: String,
) -> Result<std::result::Result<bool, String>> {
	let trimmed = label.trim();
	let label = if trimmed.is_empty() {
		None
	} else {
		Some(trimmed.to_owned())
	};
	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err("Your session expired. Reload the page.".into()));
	};
	Ok(set_label(conn.get(), account_id, label)
		.await
		.map(|()| true)
		.map_err(|e| e.user_message().to_owned()))
}
