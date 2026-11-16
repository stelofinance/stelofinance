//! Components shared by the send, deposit/withdraw and pay pages: the
//! from-account panel, the recipient chip and search hits, amount and memo.
//!
//! All state lives in runtime signals owned by the page; ids are `u64`
//! signals where `0` means "none".

use crate::domain::accounts::group_by_ledger;
use crate::domain::{MAX_MEMO_LEN, format_qty, kind_token};
use crate::module_bindings::{AccountKind, MyAccountRow, Role};
use crate::stdb::DirectoryHit;
use crate::ui::text::role_label;
use topcoat::{
	Result,
	context::Cx,
	runtime::{Event, Signal, record, signal},
	view::{View, component, view},
};

/// Static suffix of the memo counter; keep in sync with `MAX_MEMO_LEN`.
const MEMO_MAX: &str = "/64";

const _: () = assert!(MAX_MEMO_LEN == 64);

/// Class of the recipient search input (see `ui::runtime::search_input`).
pub const SEARCH_CLASS: &str = "w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400";

const ROW_ON: &str = "flex w-full cursor-pointer items-start justify-between gap-3 border-l-2 border-anakiwa bg-neutral-900 px-3 py-2.5 text-left hover:bg-neutral-900 sm:px-4";
const ROW_OFF: &str = "flex w-full cursor-pointer items-start justify-between gap-3 border-l-2 border-transparent px-3 py-2.5 text-left hover:bg-neutral-900 sm:px-4";

/// What a successful send / deposit / withdraw tells the page.
#[record]
#[derive(Clone, Debug)]
pub struct Sent {
	/// "5 Coin".
	pub qty: String,
	/// Waiting on the issuer to confirm (deposit/withdraw from a debit).
	pub pending: bool,
	/// Fresh idempotency key for the next submit.
	pub key: String,
}

/// The recipient signals shared by the form, the chip and the search hits.
#[derive(Clone)]
pub struct Recipient {
	pub id: Signal<u64>,
	pub name: Signal<String>,
	pub addr: Signal<String>,
	pub label: Signal<String>,
	pub search: Signal<String>,
}

impl Recipient {
	pub fn new(cx: &Cx, id: u64, name: String, addr: String, label: String) -> Self {
		Self {
			id: signal(cx, || id),
			name: signal(cx, || name.clone()),
			addr: signal(cx, || addr.clone()),
			label: signal(cx, || label.clone()),
			search: signal(cx, String::new),
		}
	}
}

/// Primary line + optional `#address` secondary (when the primary is `@user`).
pub fn hit_identity(hit: &DirectoryHit) -> (String, String) {
	identity(hit.primary_username.as_deref(), &hit.address)
}

fn identity(username: Option<&str>, address: &str) -> (String, String) {
	match username.map(str::trim).filter(|s| !s.is_empty()) {
		Some(user) => (format!("@{user}"), format!("#{address}")),
		None => (format!("#{address}"), String::new()),
	}
}

/// The "From" cards and picker. Picking a row on another ledger clears the
/// recipient.
#[component]
pub async fn from_panel(
	cx: &Cx,
	accounts: Vec<MyAccountRow>,
	can_pick: bool,
	from_id: Signal<u64>,
	from_ledger_id: Signal<u64>,
	from_kind: Signal<String>,
	picking: Signal<bool>,
	recipient: Recipient,
	error: Signal<String>,
) -> Result<impl View> {
	let groups: Vec<(String, Vec<MyAccountRow>)> = group_by_ledger(&accounts)
		.into_iter()
		.map(|g| {
			(
				g.ledger_name.to_owned(),
				g.accounts.into_iter().cloned().collect(),
			)
		})
		.collect();
	Ok(view! {
    <div id="from-panel">
        <p class="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-400">
            "From"
        </p>
        #[key(acc.account_id)]
        for acc in &accounts {
            selected_card(
                acc: acc,
                can_pick: can_pick,
                from_id: from_id.clone(),
                picking: picking.clone()
            )
        }
        if can_pick {
            <div
                class="overflow-hidden rounded-lg border border-neutral-800 bg-neutral-950"
                :hidden=$(!picking.get())
            >
                #[key(rows[0].ledger_id)]
                for (ledger_name, rows) in &groups {
                    <p
                        class="px-3 pt-3 pb-1 text-xs font-medium uppercase tracking-wide text-neutral-400"
                    >
                        (ledger_name.clone())
                    </p>
                    #[key(acc.account_id)]
                    for acc in rows {
                        picker_row(
                            acc: acc,
                            from_id: from_id.clone(),
                            from_ledger_id: from_ledger_id.clone(),
                            from_kind: from_kind.clone(),
                            picking: picking.clone(),
                            recipient: recipient.clone(),
                            error: error.clone()
                        )
                    }
                }
            </div>
        }
    </div>
})
}

/// The chosen account as a card; clicking it opens the picker.
#[component]
async fn selected_card(
	cx: &Cx,
	acc: &MyAccountRow,
	can_pick: bool,
	from_id: Signal<u64>,
	picking: Signal<bool>,
) -> Result<impl View> {
	let id = acc.account_id;
	Ok(view! {
    <article
        class="rounded-lg border border-neutral-800 bg-neutral-950"
        :hidden=$(if from_id.get() == id { picking.get() } else { true })
    >
        if can_pick {
            <button
                type="button"
                class="flex w-full cursor-pointer items-start justify-between gap-3 px-3 py-2.5 text-left hover:bg-neutral-900 sm:px-4 sm:py-3"
                aria-label="Change sending account"
                @click=$(|_e| picking.set(true))
            >
                from_body(acc: acc)
            </button>
        } else {
            <div
                class="flex items-start justify-between gap-3 px-3 py-2.5 sm:px-4 sm:py-3"
            >
                from_body(acc: acc)
            </div>
        }
    </article>
})
}

#[component]
async fn from_body(cx: &Cx, acc: &MyAccountRow) -> Result<impl View> {
	let qty = format_qty(acc.balance, acc.ledger_scale);
	let title = from_title(acc);
	let subtitle = from_subtitle(acc, true);
	Ok(view! {
    <div class="min-w-0">
        <p class="truncate text-lg font-medium">
            (title)
            pills(acc: acc)
        </p>
        <p class="mt-1 truncate text-sm text-neutral-300">(subtitle)</p>
    </div>
    <div class="shrink-0 text-right">
        <p class="text-2xl font-medium tabular-nums sm:text-3xl">(qty)</p>
        <p class="mt-0.5 text-xs text-neutral-400">"available"</p>
    </div>
})
}

#[component]
async fn picker_row(
	cx: &Cx,
	acc: &MyAccountRow,
	from_id: Signal<u64>,
	from_ledger_id: Signal<u64>,
	from_kind: Signal<String>,
	picking: Signal<bool>,
	recipient: Recipient,
	error: Signal<String>,
) -> Result<impl View> {
	let id = acc.account_id;
	let ledger = acc.ledger_id;
	let kind = kind_token(acc.kind).to_owned();
	let title = from_title(acc);
	let subtitle = from_subtitle(acc, false);
	let qty = format_qty(acc.balance, acc.ledger_scale);
	let Recipient {
		id: r_id,
		name: r_name,
		addr: r_addr,
		label: r_label,
		search: r_search,
	} = recipient;
	Ok(view! {
    <button
        type="button"
        :class=$(if from_id.get() == id { ROW_ON } else { ROW_OFF })
        @click=$(|_e| {
            if from_ledger_id.get() != ledger {
                r_id.set(0u64);
                r_name.set("".to_owned());
                r_addr.set("".to_owned());
                r_label.set("".to_owned());
                r_search.set("".to_owned());
            }
            from_id.set(id);
            from_ledger_id.set(ledger);
            from_kind.set(kind.clone());
            picking.set(false);
            error.set("".to_owned());
        })
    >
        <div class="min-w-0">
            <p class="truncate text-sm font-medium">
                (title)
                pills(acc: acc)
            </p>
            <p class="mt-0.5 truncate text-xs text-neutral-300">(subtitle)</p>
        </div>
        <p class="shrink-0 text-lg font-medium tabular-nums">(qty)</p>
    </button>
})
}

#[component]
async fn pills(cx: &Cx, acc: &MyAccountRow) -> Result<impl View> {
	let credit = matches!(acc.kind, AccountKind::Credit);
	Ok(view! {
    if acc.is_primary {
        " "
        <span
            class="ml-1 rounded-full bg-anakiwa/15 px-2 py-0.5 text-xs font-normal text-anakiwa"
        >
            "Primary"
        </span>
    }
    if credit {
        " "
        <span
            class="ml-1 rounded-full bg-neutral-800 px-2 py-0.5 text-xs font-normal text-neutral-300"
        >
            "Credit"
        </span>
    }
})
}

fn from_title(acc: &MyAccountRow) -> String {
	match acc
		.label
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty())
	{
		Some(label) => label.to_owned(),
		None => format!("#{}", acc.address),
	}
}

fn from_subtitle(acc: &MyAccountRow, show_ledger: bool) -> String {
	let has_label = acc
		.label
		.as_deref()
		.map(str::trim)
		.is_some_and(|s| !s.is_empty());
	let mut parts: Vec<String> = Vec::new();
	if has_label {
		parts.push(format!("#{}", acc.address));
	}
	if matches!(acc.role, Role::Owner) {
		parts.push("Yours".to_owned());
	} else {
		parts.push(format!("Shared · {}", role_label(acc.role)));
	}
	if show_ledger {
		parts.push(acc.ledger_name.clone());
	}
	parts.join(" · ")
}

/// Directory hits for a recipient search shard. `None` before a search.
/// Shown only while no recipient is picked and the search is non-empty.
#[component]
pub async fn recipient_hits(
	cx: &Cx,
	hits: Option<Vec<DirectoryHit>>,
	recipient: Recipient,
	error: Signal<String>,
) -> Result<impl View> {
	let Recipient {
		id: r_id,
		name: r_name,
		addr: r_addr,
		label: r_label,
		search: r_search,
	} = recipient;
	Ok(view! {
    if let Some(hits) = hits {
        <div
            class="mt-1 overflow-hidden rounded-md border border-neutral-800 bg-neutral-900"
            :hidden=$(if r_id.get() != 0u64 { true } else { r_search.get() == "" })
        >
            if hits.is_empty() {
                <p class="px-3 py-2 text-sm text-neutral-400">"No accounts match."</p>
            }
            #[key(hit.account_id)]
            for hit in &hits {
                hit_row(
                    hit: hit,
                    id: r_id.clone(),
                    name: r_name.clone(),
                    addr: r_addr.clone(),
                    label: r_label.clone(),
                    search: r_search.clone(),
                    error: error.clone()
                )
            }
        </div>
    }
})
}

#[component]
async fn hit_row(
	cx: &Cx,
	hit: &DirectoryHit,
	id: Signal<u64>,
	name: Signal<String>,
	addr: Signal<String>,
	label: Signal<String>,
	search: Signal<String>,
	error: Signal<String>,
) -> Result<impl View> {
	let hit_id = hit.account_id;
	let (hit_name, hit_addr) = hit_identity(hit);
	let hit_label = hit.own_label.clone().unwrap_or_default();
	let has_label = !hit_label.is_empty();
	let has_addr = !hit_addr.is_empty();
	Ok(view! {
    <button
        type="button"
        class="block w-full cursor-pointer px-3 py-2 text-left hover:bg-neutral-800"
        @click=$(|_e| {
            id.set(hit_id);
            name.set(hit_name.clone());
            addr.set(hit_addr.clone());
            label.set(hit_label.clone());
            search.set("".to_owned());
            error.set("".to_owned());
        })
    >
        if has_label {
            <p class="truncate text-sm text-neutral-100">(hit_label.clone())</p>
        }
        <p class="truncate text-sm text-neutral-100">(hit_name.clone())</p>
        if has_addr {
            <p class="truncate text-xs text-neutral-400">(hit_addr.clone())</p>
        }
    </button>
})
}

/// The picked recipient, with a Clear button.
#[component]
pub async fn recipient_chip(
	cx: &Cx,
	recipient: Recipient,
	error: Signal<String>,
) -> Result<impl View> {
	let Recipient {
		id,
		name,
		addr,
		label,
		search,
	} = recipient;
	Ok(view! {
    <div
        class="flex items-center justify-between rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2.5"
        :hidden=$(id.get() == 0u64)
    >
        <div class="min-w-0">
            <p
                class="truncate text-sm text-neutral-100"
                :hidden=$(label.get().is_empty())
            >
                $(label.get())
            </p>
            <p class="truncate text-sm text-neutral-100">$(name.get())</p>
            <p
                class="truncate text-xs text-neutral-400"
                :hidden=$(addr.get().is_empty())
            >
                $(addr.get())
            </p>
        </div>
        <button
            type="button"
            class="ml-3 shrink-0 cursor-pointer text-sm text-neutral-300 hover:text-white"
            @click=$(|_e| {
                id.set(0u64);
                name.set("".to_owned());
                addr.set("".to_owned());
                label.set("".to_owned());
                search.set("".to_owned());
                error.set("".to_owned());
            })
        >
            "Clear"
        </button>
    </div>
})
}

/// Amount input with the selected account's asset name and, for a debit,
/// its available balance.
#[component]
pub async fn amount_field(
	cx: &Cx,
	accounts: Vec<MyAccountRow>,
	from_id: Signal<u64>,
	amount: Signal<String>,
) -> Result<impl View> {
	Ok(view! {
    <p class="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-400">
        "Amount"
    </p>
    <div class="flex items-center gap-2">
        <input
            type="text"
            inputmode="decimal"
            class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
            :value=$(amount.get())
            @input=$(|e: Event| amount.set(e.target.value))
            placeholder="0"
            autocomplete="off"
        >
        #[key(acc.account_id)]
        for acc in &accounts {
            ledger_suffix(acc: acc, from_id: from_id.clone())
        }
    </div>
    #[key(acc.account_id)]
    for acc in &accounts {
        avail_hint(acc: acc, from_id: from_id.clone())
    }
})
}

#[component]
async fn ledger_suffix(cx: &Cx, acc: &MyAccountRow, from_id: Signal<u64>) -> Result<impl View> {
	let id = acc.account_id;
	Ok(view! {
    <span class="shrink-0 text-sm text-neutral-300" :hidden=$(from_id.get() != id)>
        (acc.ledger_name.clone())
    </span>
})
}

#[component]
async fn avail_hint(cx: &Cx, acc: &MyAccountRow, from_id: Signal<u64>) -> Result<impl View> {
	let id = acc.account_id;
	let debit = matches!(acc.kind, AccountKind::Debit);
	let qty = format_qty(acc.balance, acc.ledger_scale);
	Ok(view! {
    <p
        class="mt-1 text-xs text-neutral-400"
        :hidden=$(if debit { from_id.get() != id } else { true })
    >
        "Available "
        <span class="text-anakiwa">(qty)</span>
    </p>
})
}

/// Memo input with its length counter.
#[component]
pub async fn memo_field(cx: &Cx, memo: Signal<String>) -> Result<impl View> {
	Ok(view! {
    <div class="mb-2 flex items-baseline justify-between">
        <p class="text-xs font-medium uppercase tracking-wide text-neutral-400">
            "Memo "
            <span class="font-normal normal-case tracking-normal text-neutral-400">
                "(optional)"
            </span>
        </p>
        <p class="text-xs text-neutral-400">
            $(memo.get().len())
            (MEMO_MAX)
        </p>
    </div>
    <input
        type="text"
        class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
        :value=$(memo.get())
        @input=$(|e: Event| memo.set(e.target.value))
        maxlength=(MAX_MEMO_LEN)
        placeholder="What's this for?"
        autocomplete="off"
    >
})
}
