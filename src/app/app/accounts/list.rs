//! Live grouped portfolio for GET `/app/accounts` (Topcoat runtime trial).
//!
//! A shard holding one `live!` region. The HTTP render emits the current list
//! once; the browser then opens the runtime WebSocket and the shard renders
//! again with `connected(cx) == true`, re-emitting the list on every
//! `my_accounts` change. Reconnects simply render again.

use crate::auth::require_user;
use crate::domain::accounts::{LedgerGroup, group_by_ledger};
use crate::domain::format_qty;
use crate::module_bindings::{AccountKind, LedgerKind, MyAccountRow, Role};
use crate::stdb::{LiveAccounts, LiveFeed};
use crate::ui::runtime::copy_button;
use crate::ui::text::{ledger_kind_label, role_label};
use topcoat::{
	Result,
	context::Cx,
	runtime::{Signal, connected, shard},
	view::{View, component, emit, live, view},
};

/// `#accounts-list`: SSR on the page, then live over the runtime socket.
///
/// Shard endpoints skip the `/app` layout guard, so this checks the session
/// itself. `acquire_user_db` would too, but fail with a 500 instead of the
/// login redirect.
#[shard("/app/accounts/list")]
pub async fn accounts_list(
	cx: &Cx,
	creating: Signal<bool>,
	error: Signal<String>,
) -> Result<impl View> {
	require_user(cx).await?;

	Ok(live! {
    // Resubscribes itself after a SpacetimeDB restart (see `LiveFeed`).
    let mut feed = LiveFeed::new(LiveAccounts::start);
    loop {
        let rows = feed.next(cx).await?;
        let token = emit! {
            list_view(rows: rows, creating: creating.clone(), error: error.clone())
        }?;
        if !connected(cx) {
            break Ok(token);
        }
    }
})
}

#[component]
async fn list_view(
	cx: &Cx,
	rows: Vec<MyAccountRow>,
	creating: Signal<bool>,
	error: Signal<String>,
) -> Result<impl View> {
	let is_empty = rows.is_empty();
	let (debit, credit): (Vec<&MyAccountRow>, Vec<&MyAccountRow>) = rows
		.iter()
		.partition(|a| matches!(a.kind, AccountKind::Debit));
	// `view!` moves its captures, so the borrowed groups need owned copies.
	let debit = owned_groups(group_by_ledger(debit));
	let credit = owned_groups(group_by_ledger(credit));

	Ok(view! {
    <div id="accounts-list" class="flex flex-col">
        if is_empty {
            empty_state(creating: creating.clone(), error: error.clone())
        }
        #[key(group.name.clone())]
        for group in &debit {
            ledger_group(group: group)
        }
        if !credit.is_empty() {
            <h2
                class="mt-4 mb-3 text-sm font-medium uppercase tracking-wide text-neutral-400"
            >
                "Issuer accounts"
            </h2>
            #[key(group.name.clone())]
            for group in &credit {
                ledger_group(group: group)
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
async fn ledger_group(cx: &Cx, group: &Group) -> Result<impl View> {
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
            #[key(acc.account_id)]
            for acc in &group.accounts {
                account_card(acc: acc)
            }
        </div>
    </section>
})
}

#[component]
async fn account_card(cx: &Cx, acc: &MyAccountRow) -> Result<impl View> {
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
        copy_button(
            text: $(address),
            class: "shrink-0 cursor-pointer self-stretch border-l border-neutral-800 px-3 text-xs text-neutral-300 hover:bg-neutral-900 hover:text-white",
            aria_label: Some(format!("Copy address {}", acc.address))
        )
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

/// Shown when the user has no accounts and the create form is closed. The
/// signals are the page's; they are only used inside `$(...)`, never read on
/// the server, so opening the form does not re-render the live list.
#[component]
async fn empty_state(cx: &Cx, creating: Signal<bool>, error: Signal<String>) -> Result<impl View> {
	Ok(view! {
    <div
        class="rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center"
        :hidden=$(creating.get())
    >
        <p class="text-neutral-300">"You don't have any accounts yet."</p>
        <p class="mt-2 text-sm text-neutral-400">
            "Create one to hold assets and receive from other players."
        </p>
        <button
            type="button"
            class="mt-6 rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
            @click=$(|_e| {
                creating.set(true);
                error.set("".to_owned());
            })
        >
            "Create an account"
        </button>
    </div>
})
}
