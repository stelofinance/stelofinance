//! `GET /app/accounts` and the create procedure. Live list: the `list` shard.

mod account_id;
mod list;

use crate::auth::require_user;
use crate::domain::{has_primary_on_ledger, parse_kind};
use crate::module_bindings::{AccountKind, Ledger};
use crate::stdb::{acquire_user_db, create_user_account, fetch_accounts_page};
use crate::ui::text::ledger_kind_label;
use list::accounts_list;
use topcoat::{
	Result,
	context::Cx,
	metadata::Metadata,
	router::{error::internal_server_error, page},
	runtime::{Event, Signal, expr, procedure, signal},
	view::{View, ViewExt, component, view},
};

const CHOICE_ON: &str = "flex w-full cursor-pointer items-start justify-between gap-3 rounded-lg border border-anakiwa bg-anakiwa-900 p-4 text-left transition-colors hover:border-neutral-600";
const CHOICE_OFF: &str = "flex w-full cursor-pointer items-start justify-between gap-3 rounded-lg border border-neutral-800 bg-neutral-900 p-4 text-left transition-colors hover:border-neutral-600";

/// `GET /app/accounts` — SSR current `my_accounts` + create sheet.
#[page(with(Metadata::new().title("Accounts · Stelo Finance")))]
async fn page(cx: &Cx) -> Result<impl View> {
	let user = require_user(cx).await?;
	let conn = acquire_user_db(cx).await?;
	let data = fetch_accounts_page(conn.get())
		.await
		.map_err(internal_server_error)?;

	let primary_ids = primary_set(
		data.accounts
			.iter()
			.filter(|a| a.is_primary)
			.map(|a| a.ledger_id),
	);

	Ok(view! {
    <main
        id="page-content"
        class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
    >
        accounts_view(
            ledgers: data.ledgers,
            username: user.bitcraft_username.clone(),
            is_admin: user.is_admin,
            primary_ids: primary_ids
        )
    </main>
})
}

/// Owns the create-form signals and wires the header, form and live list.
#[component]
async fn accounts_view(
	cx: &Cx,
	ledgers: Vec<Ledger>,
	username: String,
	is_admin: bool,
	primary_ids: String,
) -> Result<impl View> {
	// A lone ledger is preselected.
	let initial_ledger_id = if ledgers.len() == 1 { ledgers[0].id } else { 0 };
	let can_create = !ledgers.is_empty();
	let creating = signal(cx, || false);
	let ledger_id = signal(cx, || initial_ledger_id);
	let kind = signal(cx, || "debit".to_owned());
	let address = signal(cx, String::new);
	let label = signal(cx, String::new);
	let error = signal(cx, String::new);
	let busy = signal(cx, || false);
	let primary_ledgers = signal(cx, || primary_ids.clone());

	Ok(view! {
    page_header(
        can_create: can_create,
        initial_ledger_id: initial_ledger_id,
        creating: creating.clone(),
        ledger_id: ledger_id.clone(),
        kind: kind.clone(),
        address: address.clone(),
        label: label.clone(),
        error: error.clone()
    )
    create_form(
        ledgers: ledgers,
        username: username,
        is_admin: is_admin,
        creating: creating.clone(),
        ledger_id: ledger_id.clone(),
        kind: kind.clone(),
        address: address.clone(),
        label: label.clone(),
        error: error.clone(),
        busy: busy.clone(),
        primary_ledgers: primary_ledgers.clone()
    )
    accounts_list(creating: creating.clone(), error: error.clone())
})
}

/// Create an account. The live `accounts_list` shard shows the new row.
///
/// Failures come back as `Err(message)` data (a procedure `Err` would only
/// fail the browser expression and leave `busy` stuck). `Ok` carries the
/// user's primary ledger ids after the create, for the form's helper text.
#[procedure("/app/accounts/create")]
async fn create(
	cx: &Cx,
	ledger_id: u64,
	kind: String,
	address: String,
	label: String,
) -> Result<std::result::Result<String, String>> {
	let Ok(user) = require_user(cx).await else {
		return Ok(Err("Your session expired. Reload the page.".into()));
	};
	if ledger_id == 0 {
		return Ok(Err("Choose an asset.".into()));
	}

	let kind = match parse_kind(&kind) {
		Some(AccountKind::Credit) if user.is_admin => AccountKind::Credit,
		Some(AccountKind::Credit) => {
			return Ok(Err(
				"Only platform admins can create credit accounts.".into()
			));
		}
		_ => AccountKind::Debit,
	};

	let address = address.trim();
	let address = if address.is_empty() {
		None
	} else if user.is_admin {
		Some(address.to_owned())
	} else {
		return Ok(Err("Custom address is admin-only.".into()));
	};

	let label = label.trim();
	let label = if label.is_empty() {
		None
	} else {
		Some(label.to_owned())
	};

	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err("Your session expired. Reload the page.".into()));
	};
	let snapshot = match fetch_accounts_page(conn.get()).await {
		Ok(p) => p,
		Err(e) => return Ok(Err(e.user_message().to_owned())),
	};
	let is_primary =
		matches!(kind, AccountKind::Debit) && !has_primary_on_ledger(&snapshot.accounts, ledger_id);

	match create_user_account(conn.get(), ledger_id, kind, address, label, is_primary).await {
		Ok(()) => {
			let ids = snapshot
				.accounts
				.iter()
				.filter(|a| a.is_primary)
				.map(|a| a.ledger_id)
				.chain(is_primary.then_some(ledger_id));
			Ok(Ok(primary_set(ids)))
		}
		Err(e) => Ok(Err(e.user_message().to_owned())),
	}
}

#[component]
async fn page_header(
	cx: &Cx,
	can_create: bool,
	initial_ledger_id: u64,
	creating: Signal<bool>,
	ledger_id: Signal<u64>,
	kind: Signal<String>,
	address: Signal<String>,
	label: Signal<String>,
	error: Signal<String>,
) -> Result<impl View> {
	Ok(view! {
    <div class="mb-6 flex items-center justify-between gap-3">
        <h1 class="text-2xl font-medium md:text-3xl">"Accounts"</h1>
        if can_create {
            <button
                type="button"
                class="rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
                :hidden=$(creating.get())
                @click=$(|_e| {
                    creating.set(true);
                    error.set("".to_owned());
                    ledger_id.set(initial_ledger_id);
                    kind.set("debit".to_owned());
                    address.set("".to_owned());
                    label.set("".to_owned());
                })
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
	creating: Signal<bool>,
	ledger_id: Signal<u64>,
	kind: Signal<String>,
	address: Signal<String>,
	label: Signal<String>,
	error: Signal<String>,
	busy: Signal<bool>,
	primary_ledgers: Signal<String>,
) -> Result<impl View> {
	if ledgers.is_empty() {
		return Ok(view! {
    <p class="mt-2 text-sm text-neutral-400">"No assets are on the platform yet."</p>
}
		.boxed());
	}

	Ok(view! {
    <div
        :hidden=$(!creating.get())
        class="mb-6 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
    >
        <h2 class="text-lg font-medium">"New account"</h2>
        <p class="mt-1 text-sm text-neutral-300">
            "Choose the asset this account will hold."
        </p>

        <div class="mt-4 grid grid-cols-1 gap-2 sm:grid-cols-2">
            #[key(ledger.id)]
            for ledger in &ledgers {
                ledger_choice(
                    ledger: ledger,
                    ledger_id: ledger_id.clone(),
                    error: error.clone()
                )
            }
        </div>

        #[key(ledger.id)]
        for ledger in &ledgers {
            create_helper(
                ledger: ledger,
                username: username.clone(),
                ledger_id: ledger_id.clone(),
                kind: kind.clone(),
                primary_ledgers: primary_ledgers.clone()
            )
        }

        <p
            class="mt-3 text-sm text-neutral-300"
            :hidden=$(if kind.get() == "credit" {
                ledger_id.get() == 0u64
            } else {
                true
            })
        >
            "Credit accounts sit on the issuer side of this asset. They cannot be primary."
        </p>

        <label class="mt-4 block">
            <span class="text-sm text-neutral-300">"Label "</span>
            <span class="text-sm text-neutral-400">"(optional)"</span>
            <input
                type="text"
                class="mt-1 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
                :value=$(label.get())
                @input=$(|e: Event| label.set(e.target.value))
                maxlength="32"
                placeholder="e.g. Guild treasury"
                autocomplete="off"
            >
        </label>

        if is_admin {
            admin_advanced(kind: kind.clone(), address: address.clone())
        }

        <p class="mt-3 text-sm text-red-400" :hidden=$(error.get().is_empty())>
            $(error.get())
        </p>

        <div class="mt-5 flex justify-end gap-2">
            <button
                type="button"
                class="rounded-md border border-neutral-700 px-4 py-2 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white"
                @click=$(|_e| {
                    creating.set(false);
                    error.set("".to_owned());
                })
            >
                "Cancel"
            </button>
            <button
                type="button"
                class="rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
                :disabled=$(if busy.get() { true } else { ledger_id.get() == 0u64 })
                @click=$(async |_e| {
                    busy.set(true);
                    let result = create(
                        ledger_id.get(),
                        kind.get(),
                        address.get(),
                        label.get(),
                    ).await;
                    busy.set(false);
                    if result.is_ok() {
                        primary_ledgers.set(result.unwrap());
                        creating.set(false);
                        error.set("".to_owned());
                        kind.set("debit".to_owned());
                        address.set("".to_owned());
                        label.set("".to_owned());
                    } else {
                        error.set(result.unwrap_err());
                    }
                })
            >
                "Create"
            </button>
        </div>
    </div>
}.boxed())
}

#[component]
async fn ledger_choice(
	cx: &Cx,
	ledger: &Ledger,
	ledger_id: Signal<u64>,
	error: Signal<String>,
) -> Result<impl View> {
	let id = ledger.id;
	let kind = ledger_kind_label(ledger.kind);

	Ok(view! {
    <button
        type="button"
        :class=$(if ledger_id.get() == id { CHOICE_ON } else { CHOICE_OFF })
        @click=$(|_e| {
            ledger_id.set(id);
            error.set("".to_owned());
        })
    >
        <div class="min-w-0">
            <p class="font-medium">(ledger.name.clone())</p>
            <p class="mt-0.5 text-xs text-neutral-400">(kind)</p>
        </div>
        <span
            class="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border-2 border-neutral-600"
            :hidden=$(ledger_id.get() == id)
        ></span>
        <span
            class="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border-2 border-anakiwa bg-anakiwa text-xs font-bold text-neutral-900"
            :hidden=$(ledger_id.get() != id)
        >
            "✓"
        </span>
    </button>
})
}

#[component]
async fn create_helper(
	cx: &Cx,
	ledger: &Ledger,
	username: String,
	ledger_id: Signal<u64>,
	kind: Signal<String>,
	primary_ledgers: Signal<String>,
) -> Result<impl View> {
	let id = ledger.id;
	let first = format!(
		"This will be your primary {} account. People can send to @{username}.",
		ledger.name
	);
	let existing = format!("Friends sending to @{username} still go to your primary.");
	let needle = primary_needle(id);
	let needle = needle.as_str();
	let has_primary = expr!(primary_ledgers.get().contains(needle));
	let selected = expr!(if ledger_id.get() == id {
		kind.get() != "credit"
	} else {
		false
	});

	Ok(view! {
    <p
        class="mt-3 text-sm text-neutral-300"
        :hidden=$(if selected { has_primary } else { true })
    >
        (first)
    </p>
    <p
        class="mt-3 text-sm text-neutral-300"
        :hidden=$(if selected { !has_primary } else { true })
    >
        (existing)
    </p>
})
}

#[component]
async fn admin_advanced(
	cx: &Cx,
	kind: Signal<String>,
	address: Signal<String>,
) -> Result<impl View> {
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
                    :value=$(kind.get())
                    @change=$(|e: Event| kind.set(e.target.value))
                >
                    <option value="debit" selected="">"Debit"</option>
                    <option value="credit">"Credit (issuer)"</option>
                </select>
            </label>
            <label class="block">
                <span class="text-sm text-neutral-300">"Custom address "</span>
                <span class="text-sm text-neutral-400">"(optional)"</span>
                <input
                    type="text"
                    class="mt-1 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm uppercase placeholder:text-neutral-400"
                    :value=$(address.get())
                    @input=$(|e: Event| address.set(e.target.value))
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

/// Ledger ids where the user has a primary account, as `"|1|2|"`. The
/// expression vocabulary has `String::contains` but no `Vec` membership, so
/// the helper text tests `contains("|<id>|")`.
fn primary_set(ids: impl Iterator<Item = u64>) -> String {
	let mut set = String::from("|");
	for id in ids {
		set.push_str(&id.to_string());
		set.push('|');
	}
	set
}

fn primary_needle(ledger_id: u64) -> String {
	format!("|{ledger_id}|")
}
