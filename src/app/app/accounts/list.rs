//! Live grouped portfolio for GET `/app/accounts` (Topcoat runtime trial).
//!
//! A shard holding one `live!` region. The HTTP render emits the current list
//! once; the browser then opens the runtime WebSocket and the shard renders
//! again with `connected(cx) == true`, re-emitting the list on every
//! `my_accounts` change. Reconnects simply render again.

use std::time::Duration;

use crate::auth::require_user;
use crate::domain::accounts::{LedgerGroup, group_by_ledger};
use crate::domain::format_qty;
use crate::module_bindings::{AccountKind, LedgerKind, MyAccountRow, Role};
use crate::stdb::{LiveAccounts, acquire_user_db, next_change};
use crate::ui::text::{ledger_kind_label, role_label};
use topcoat::{
	Result,
	context::Cx,
	router::error::internal_server_error,
	runtime::{Signal, connected, shard, signal},
	view::{View, component, emit, live, view},
};

/// `#accounts-list`: SSR on the page, then live over the runtime socket.
///
/// Shard endpoints skip the `/app` layout guard, so this checks the session
/// itself. `acquire_user_db` would too, but fail with a 500 instead of the
/// login redirect.
#[shard("/app/accounts/list")]
pub async fn accounts_list(cx: &Cx) -> Result<impl View> {
	require_user(cx).await?;
	let copied = signal(cx, || 0u64);

	Ok(live! {
    let mut retry = Duration::from_millis(250);
    loop {
        let conn = acquire_user_db(cx).await?;
        // Latest-wins slot: one transaction's row callbacks collapse to one emit.
        let (tx, mut rx) = tokio::sync::watch::channel(None);
        let live = LiveAccounts::start(conn, tx).map_err(internal_server_error)?;
        // The first value is the applied snapshot, so HTTP and connected
        // renders share this loop. `None`: the STDB connection dropped.
        while next_change(&mut rx, live.conn()).await.is_some() {
            let rows = rx.borrow_and_update().clone().unwrap_or_default();
            let token = emit! { list_view(rows: rows, copied: copied.clone()) }?;
            if !connected(cx) {
                return Ok(token);
            }
            retry = Duration::from_millis(250);
        }
        // The runtime never restarts a finished run (only a closed socket
        // reconnects), so resubscribe here on a fresh pooled connection.
        drop(live);
        if !connected(cx) {
            return Err(
                internal_server_error(
                    crate::stdb::StdbError::internal(
                        "stdb connection closed before the accounts snapshot",
                    ),
                ).into(),
            );
        }
        tokio::time::sleep(retry).await;
        retry = (retry * 2).min(Duration::from_secs(10));
    }
})
}

#[component]
async fn list_view(cx: &Cx, rows: Vec<MyAccountRow>, copied: Signal<u64>) -> Result<impl View> {
	let is_empty = rows.is_empty();
	// Bridge to the Datastar create form's `$primaryIds` (trial only).
	// Datastar re-applies `data-signals` when the runtime morphs this attribute.
	let primary_ids =
		super::encode_primary_ids(rows.iter().filter(|a| a.is_primary).map(|a| a.ledger_id));
	let primary_signal = format!("{{primaryIds:'{primary_ids}'}}");
	let (debit, credit): (Vec<&MyAccountRow>, Vec<&MyAccountRow>) = rows
		.iter()
		.partition(|a| matches!(a.kind, AccountKind::Debit));
	// `view!` moves its captures, so the borrowed groups need owned copies.
	let debit = owned_groups(group_by_ledger(debit));
	let credit = owned_groups(group_by_ledger(credit));

	Ok(view! {
    <div id="accounts-list" class="flex flex-col">
        <div class="hidden" data-signals=(primary_signal)></div>
        if is_empty {
            empty_state()
        }
        for group in &debit {
            ledger_group(group: group, copied: copied.clone())
        }
        if !credit.is_empty() {
            <h2
                class="mt-4 mb-3 text-sm font-medium uppercase tracking-wide text-neutral-400"
            >
                "Issuer accounts"
            </h2>
            for group in &credit {
                ledger_group(group: group, copied: copied.clone())
            }
        }
    </div>
})
}

struct Group {
	name: String,
	kind: LedgerKind,
	accounts: Vec<MyAccountRow>,
}

fn owned_groups(groups: Vec<LedgerGroup<'_>>) -> Vec<Group> {
	groups
		.into_iter()
		.map(|g| Group {
			name: g.ledger_name.to_owned(),
			kind: g.ledger_kind,
			accounts: g.accounts.into_iter().cloned().collect(),
		})
		.collect()
}

#[component]
async fn ledger_group(cx: &Cx, group: &Group, copied: Signal<u64>) -> Result<impl View> {
	Ok(view! {
    <section class="mb-5 last:mb-0">
        <div class="mb-3 flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
            <h2 class="text-sm font-medium uppercase tracking-wide text-neutral-300">
                (group.name.clone())
            </h2>
            <span class="text-xs text-neutral-400">
                (ledger_kind_label(group.kind))
            </span>
        </div>
        <div class="flex flex-col gap-3">
            for acc in &group.accounts {
                account_card(acc: acc, copied: copied.clone())
            }
        </div>
    </section>
})
}

#[component]
async fn account_card(cx: &Cx, acc: &MyAccountRow, copied: Signal<u64>) -> Result<impl View> {
	let qty = format_qty(acc.balance, acc.ledger_scale);
	let href = format!("/app/accounts/{}", acc.account_id);
	let label = acc
		.label
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty());
	let aria = match label {
		Some(l) => format!("{l}, {qty}"),
		None => format!("#{}, {qty}", acc.address),
	};
	let id = acc.account_id;
	let address = acc.address.clone();

	Ok(view! {
    <article
        id=(format!("account-{id}"))
        class="flex overflow-hidden rounded-lg border border-neutral-800 bg-neutral-950 transition-colors hover:border-neutral-600"
    >
        <a
            href=(href)
            class="min-w-0 flex-1 px-3 py-2.5 focus:outline-none focus-visible:ring-2 focus-visible:ring-anakiwa/50 sm:px-4 sm:py-3"
            aria-label=(aria)
        >
            <div class="flex items-start justify-between gap-3">
                <p class="text-xl font-medium tabular-nums sm:text-2xl">(qty)</p>
                if acc.is_primary {
                    <span
                        class="shrink-0 rounded-full bg-anakiwa/15 px-2.5 py-0.5 text-xs text-anakiwa"
                    >
                        "Primary"
                    </span>
                } else if matches!(acc.kind, AccountKind::Credit) {
                    <span
                        class="shrink-0 rounded-full bg-neutral-800 px-2.5 py-0.5 text-xs text-neutral-300"
                    >
                        "Credit"
                    </span>
                }
            </div>
            if let Some(label) = label {
                <p class="mt-1 truncate text-sm text-neutral-200">(label)</p>
            }
            <p class="mt-1 truncate text-sm text-neutral-300">
                "#"
                (acc.address.clone())
                (meta_suffix(acc))
            </p>
        </a>
        <button
            type="button"
            class="shrink-0 cursor-pointer self-stretch border-l border-neutral-800 px-3 text-xs text-neutral-300 hover:bg-neutral-900 hover:text-white"
            aria-label=(format!("Copy address {}", acc.address))
            @click=$(|_e| {
                // Clipboard and timers are outside the expression vocabulary.
                // JS sees runtime surrogates: `.toString()` unwraps a string,
                // and a signal is set with another surrogate (`zero`).
                copied.set(id);
                let zero = 0u64;
                raw!(
                    "navigator.clipboard.writeText(${address}.toString()); setTimeout(() => ${copied}.set(${zero}), 2000)",
                    {
                        let _ = zero;
                    },
                )
            })
        >
            $(if copied.get() == id { "Copied" } else { "Copy" })
        </button>
    </article>
})
}

fn meta_suffix(acc: &MyAccountRow) -> String {
	if matches!(acc.role, Role::Owner) {
		return " · Yours".to_owned();
	}
	let mut s = format!(" · Shared · {}", role_label(acc.role));
	if let Some(owner) = acc
		.owner_username
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty())
	{
		s.push_str(" · owned by ");
		s.push_str(owner);
	}
	s
}

/// Uses the page's Datastar `$creatingAcc` (the create form stays on Datastar
/// in this trial); Datastar binds these attributes after each runtime morph.
#[component]
async fn empty_state(cx: &Cx) -> Result<impl View> {
	Ok(view! {
    <div
        class="rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center"
        data-show="!$creatingAcc"
    >
        <p class="text-neutral-300">"You don't have any accounts yet."</p>
        <p class="mt-2 text-sm text-neutral-400">
            "Create one to hold assets and receive from other players."
        </p>
        <button
            type="button"
            class="mt-6 rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
            data-on:click="$creatingAcc = true; $createError = ''"
        >
            "Create an account"
        </button>
    </div>
})
}
