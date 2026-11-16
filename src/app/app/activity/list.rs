//! Live filter chips + day-grouped cards (the `activity_body` shard).
//!
//! The filter is one `Signal<String>` holding the selected account as a
//! needle, `"|12|"`, or `""` for All. The shard only reads it inside `$(...)`,
//! so switching chips is a pure client-side update. A card is visible when its
//! account set (`"|1|2|"`) `contains` the needle: `""` is contained in every
//! set, so All needs no special case.

use super::finalize::finalize_transfer;
use super::present::{AmountSign, Line, TransferCard, account_title, transfer_card};
use crate::auth::require_user;
use crate::domain::{day_heading, format_rel_time, unix_now_micros};
use crate::module_bindings::MyAccountRow;
use crate::stdb::{ActivitySnapshot, LiveActivity, LiveFeed};
use std::collections::BTreeSet;
use topcoat::{
	Result,
	context::Cx,
	runtime::{Signal, connected, expr, shard},
	view::{View, component, emit, live, view},
};

const CHIP_ON: &str = "cursor-pointer rounded-full px-3 py-1.5 text-sm bg-anakiwa-700 text-white hover:bg-neutral-800 hover:text-white";
const CHIP_OFF: &str = "cursor-pointer rounded-full px-3 py-1.5 text-sm text-neutral-300 hover:bg-neutral-800 hover:text-white";

/// `#activity-body`: SSR on the page, then live over the runtime socket.
///
/// Shard endpoints skip the `/app` layout guard, so this checks the session
/// itself. `filter` is deliberately never read on the server.
#[shard("/app/activity/body")]
pub async fn activity_body(
	cx: &Cx,
	filter: Signal<String>,
	busy: Signal<bool>,
	error: Signal<String>,
) -> Result<impl View> {
	require_user(cx).await?;

	Ok(live! {
    let mut feed = LiveFeed::new(LiveActivity::start);
    loop {
        let data = feed.next(cx).await?;
        let rendered = emit! {
            body_view(
                data: data,
                filter: filter.clone(),
                busy: busy.clone(),
                error: error.clone()
            )
        }?;
        if !connected(cx) {
            break Ok(rendered);
        }
    }
})
}

/// `"|12|"`: the filter value for one account. `""` is All.
pub fn needle(account_id: u64) -> String {
	format!("|{account_id}|")
}

/// Account ids as a `"|1|2|"` set, sorted and deduplicated.
fn id_set(ids: impl IntoIterator<Item = u64>) -> String {
	let ids: BTreeSet<u64> = ids.into_iter().collect();
	let mut set = String::from("|");
	for id in ids {
		set.push_str(&id.to_string());
		set.push('|');
	}
	set
}

/// Consecutive cards under one day heading.
struct DayGroup {
	heading: String,
	/// Accounts involved in any card of the day.
	set: String,
	cards: Vec<TransferCard>,
}

fn group_days(cards: Vec<TransferCard>, now_micros: i64) -> Vec<DayGroup> {
	let mut groups: Vec<DayGroup> = Vec::new();
	for card in cards {
		let heading = day_heading(card.created_micros, now_micros);
		match groups.last_mut() {
			Some(g) if g.heading == heading => g.cards.push(card),
			_ => groups.push(DayGroup {
				heading,
				set: String::new(),
				cards: vec![card],
			}),
		}
	}
	for g in &mut groups {
		g.set = id_set(g.cards.iter().flat_map(|c| c.involved.iter().copied()));
	}
	groups
}

#[component]
async fn body_view(
	cx: &Cx,
	data: ActivitySnapshot,
	filter: Signal<String>,
	busy: Signal<bool>,
	error: Signal<String>,
) -> Result<impl View> {
	let now = unix_now_micros();
	let cards: Vec<TransferCard> = data
		.transfers
		.iter()
		.filter_map(|tr| transfer_card(tr, &data.accounts))
		.collect();

	// Accounts with no transfers get a "none yet" block when selected.
	let active: BTreeSet<u64> = cards
		.iter()
		.flat_map(|c| c.involved.iter().copied())
		.collect();
	let quiet = id_set(
		data.accounts
			.iter()
			.map(|a| a.account_id)
			.filter(|id| !active.contains(id)),
	);
	let has_quiet = quiet != "|";
	let has_accounts = !data.accounts.is_empty();
	let has_cards = !cards.is_empty();
	let quiet = quiet.as_str();
	let quiet_hidden = expr!(if filter.get() == "" {
		true
	} else {
		!quiet.contains(filter.get().trim())
	});
	let groups = group_days(cards, now);

	Ok(view! {
    <div id="activity-body">
        if !has_accounts {
            empty_state(
                title: "Nothing here yet.",
                blurb: "Create an account to send and receive.",
                show_send: false
            )
        } else {
            chips(accounts: data.accounts.clone(), filter: filter.clone())
            if !has_cards {
                empty_state(
                    title: "Nothing here yet.",
                    blurb: "Send or receive to see activity.",
                    show_send: true
                )
            } else {
                if has_quiet {
                    <div
                        class="rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center"
                        :hidden=(quiet_hidden)
                    >
                        <p class="text-neutral-300">
                            "No transfers on this account yet."
                        </p>
                    </div>
                }
                <div id="activity-list" class="flex flex-col">
                    #[key(g.heading.clone())]
                    for g in &groups {
                        day_section(
                            group: g,
                            now: now,
                            filter: filter.clone(),
                            busy: busy.clone(),
                            error: error.clone()
                        )
                    }
                </div>
            }
        }
    </div>
})
}

#[component]
async fn chips(cx: &Cx, accounts: Vec<MyAccountRow>, filter: Signal<String>) -> Result<impl View> {
	Ok(view! {
    <div
        class="mb-6 flex flex-wrap gap-2"
        role="tablist"
        aria-label="Filter by account"
    >
        chip(
            label: "All".to_owned(),
            needle: String::new(),
            url: "/app/activity".to_owned(),
            filter: filter.clone()
        )
        #[key(acc.account_id)]
        for acc in &accounts {
            chip(
                label: account_title(acc),
                needle: needle(acc.account_id),
                url: format!("/app/activity?account={}", acc.account_id),
                filter: filter.clone()
            )
        }
    </div>
})
}

/// Sets the filter and updates the URL without navigating.
#[component]
async fn chip(
	cx: &Cx,
	label: String,
	needle: String,
	url: String,
	filter: Signal<String>,
) -> Result<impl View> {
	let n = needle.as_str();
	let on = expr!(filter.get() == n);
	Ok(view! {
    <button
        type="button"
        role="tab"
        class=(CHIP_OFF)
        :class=$(if on { CHIP_ON } else { CHIP_OFF })
        :aria-pressed=$(if on { "true" } else { "false" })
        @click=$(|_e| {
            filter.set(needle.clone());
            raw!(
                "window.history.replaceState(null, '', ${url}.toString())",
                {
                    let _ = url;
                },
            )
        })
    >
        (label)
    </button>
})
}

#[component]
async fn day_section(
	cx: &Cx,
	group: &DayGroup,
	now: i64,
	filter: Signal<String>,
	busy: Signal<bool>,
	error: Signal<String>,
) -> Result<impl View> {
	let set = group.set.as_str();
	let hidden = expr!(!set.contains(filter.get().trim()));
	Ok(view! {
    <section class="mb-5 last:mb-0" :hidden=(hidden)>
        <h2 class="mb-3 text-sm font-medium uppercase tracking-wide text-neutral-400">
            (group.heading.clone())
        </h2>
        <div class="flex flex-col gap-3">
            #[key(card.id)]
            for card in &group.cards {
                card_view(
                    card: card,
                    now: now,
                    filter: filter.clone(),
                    busy: busy.clone(),
                    error: error.clone()
                )
            }
        </div>
    </section>
})
}

#[component]
async fn card_view(
	cx: &Cx,
	card: &TransferCard,
	now: i64,
	filter: Signal<String>,
	busy: Signal<bool>,
	error: Signal<String>,
) -> Result<impl View> {
	let set = id_set(card.involved.iter().copied());
	let set = set.as_str();
	let hidden = expr!(!set.contains(filter.get().trim()));
	Ok(view! {
    <article
        class="rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2.5 sm:px-4 sm:py-3"
        data-transfer-id=(card.id.to_string())
        :hidden=(hidden)
    >
        <div :hidden=$(filter.get() != "")>
            line_view(card: card, line: &card.all, now: now)
        </div>
        #[key(id)]
        for (id, line) in &card.per_account {
            card_line(
                card: card,
                line: line,
                needle: needle(*id),
                now: now,
                filter: filter.clone()
            )
        }
        if card.can_finalize {
            finalize_buttons(
                transfer_id: card.id,
                busy: busy.clone(),
                error: error.clone()
            )
        }
    </article>
})
}

/// One account's reading of a card, shown while that account is selected.
#[component]
async fn card_line(
	cx: &Cx,
	card: &TransferCard,
	line: &Line,
	needle: String,
	now: i64,
	filter: Signal<String>,
) -> Result<impl View> {
	let needle = needle.as_str();
	let off = expr!(filter.get() != needle);
	Ok(view! { <div :hidden=(off)>line_view(card: card, line: line, now: now)</div> })
}

#[component]
async fn line_view(cx: &Cx, card: &TransferCard, line: &Line, now: i64) -> Result<impl View> {
	// Void has no balance effect on any filter. Strike the historical hold,
	// grey, with no "+" or "-" sign.
	let (qty, qty_class) = match (&card.voided_qty, line.sign) {
		(Some(_), _) => (
			String::new(),
			"shrink-0 text-xl font-medium tabular-nums text-neutral-400 sm:text-2xl",
		),
		(None, AmountSign::In) => (
			format!("+{}", card.qty),
			"shrink-0 text-xl font-medium tabular-nums text-anakiwa sm:text-2xl",
		),
		(None, AmountSign::Out) => (
			format!("−{}", card.qty),
			"shrink-0 text-xl font-medium tabular-nums sm:text-2xl",
		),
		(None, AmountSign::Flat) => (
			card.qty.clone(),
			"shrink-0 text-xl font-medium tabular-nums sm:text-2xl",
		),
	};
	let rel = format_rel_time(card.created_micros, now);

	Ok(view! {
    <div class="flex items-start justify-between gap-3">
        <p class="min-w-0 truncate text-sm font-medium">(line.title.clone())</p>
        <p class=(qty_class)>
            if let Some(held) = &card.voided_qty {
                <span class="line-through">(held.clone())</span>
            } else {
                (qty)
            }
        </p>
    </div>
    <div class="mt-0.5 flex items-baseline justify-between gap-3">
        <p class="min-w-0 truncate text-sm text-neutral-300">
            (line.subtitle.clone())
            if let Some(state) = card.state_label {
                " "
                <span
                    class="ml-1 rounded-full bg-neutral-800 px-2 py-0.5 text-xs text-neutral-300"
                >
                    (state)
                </span>
            }
        </p>
        <p class="shrink-0 text-xs text-neutral-400">(card.ledger_name.clone())</p>
    </div>
    <p class="mt-1 truncate text-xs text-neutral-400">
        (rel)
        if let Some(memo) = &card.memo {
            " · "
            (memo.clone())
        }
    </p>
})
}

#[component]
async fn finalize_buttons(
	cx: &Cx,
	transfer_id: u64,
	busy: Signal<bool>,
	error: Signal<String>,
) -> Result<impl View> {
	Ok(view! {
    <div class="mt-3 flex flex-wrap gap-2">
        <button
            type="button"
            class="cursor-pointer rounded-md bg-anakiwa-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled=$(busy.get())
            @click=$(async |_e| {
                error.set("".to_owned());
                busy.set(true);
                let result = finalize_transfer(transfer_id, true).await;
                busy.set(false);
                if result.is_err() {
                    error.set(result.unwrap_err());
                }
            })
        >
            "Confirm"
        </button>
        <button
            type="button"
            class="cursor-pointer rounded-md border border-neutral-700 px-3 py-1.5 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white disabled:cursor-not-allowed disabled:opacity-50"
            :disabled=$(busy.get())
            @click=$(async |_e| {
                error.set("".to_owned());
                busy.set(true);
                let result = finalize_transfer(transfer_id, false).await;
                busy.set(false);
                if result.is_err() {
                    error.set(result.unwrap_err());
                }
            })
        >
            "Void"
        </button>
    </div>
})
}

#[component]
async fn empty_state(
	cx: &Cx,
	title: &'static str,
	blurb: &'static str,
	show_send: bool,
) -> Result<impl View> {
	Ok(view! {
    <div
        class="rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center"
    >
        <p class="text-neutral-300">(title)</p>
        <p class="mt-2 text-sm text-neutral-400">(blurb)</p>
        <div class="mt-6 flex flex-wrap justify-center gap-3">
            if show_send {
                <a
                    href="/app/transfer"
                    class="inline-flex rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
                >
                    "Send"
                </a>
            }
            <a
                href="/app/accounts"
                class="inline-flex rounded-md border border-neutral-600 px-4 py-2 text-sm text-neutral-200 hover:border-neutral-400"
            >
                "Accounts"
            </a>
        </div>
    </div>
})
}

#[cfg(test)]
mod tests {
	use super::{group_days, id_set, needle};
	use crate::app::app::activity::present::{TransferCard, transfer_card};
	use crate::module_bindings::{
		AccountKind, LedgerKind, MyAccountRow, MyTransferRow, Role, TransferKind, TransferState,
	};
	use spacetimedb_sdk::Timestamp;

	fn ts() -> Timestamp {
		Timestamp::from_micros_since_unix_epoch(1_700_000_000_000_000)
	}

	fn account(id: u64) -> MyAccountRow {
		MyAccountRow {
			account_id: id,
			address: format!("addr{id}"),
			label: Some(format!("Acct {id}")),
			kind: AccountKind::Debit,
			balance: 0,
			ledger_id: 1,
			ledger_name: "USD".into(),
			ledger_scale: 2,
			ledger_kind: LedgerKind::Digital,
			role: Role::Owner,
			is_primary: id == 1,
			owner_username: Some("me".into()),
			webhook: None,
			created_at: ts(),
		}
	}

	fn transfer(id: u64, state: TransferState, pending: u64, posted: Option<u64>) -> MyTransferRow {
		MyTransferRow {
			id,
			debit_account_id: 2,
			credit_account_id: 1,
			debit_address: "addr2".into(),
			credit_address: "addr1".into(),
			debit_username: None,
			credit_username: None,
			pending_amount: Some(pending),
			posted_amount: posted,
			ledger_id: 1,
			ledger_name: "USD".into(),
			ledger_scale: 2,
			kind: TransferKind::Asset,
			state,
			memo: None,
			created_at: ts(),
			finalized_at: None,
		}
	}

	fn cards(transfers: Vec<MyTransferRow>) -> Vec<TransferCard> {
		let accounts = vec![account(1), account(2)];
		transfers
			.iter()
			.filter_map(|tr| transfer_card(tr, &accounts))
			.collect()
	}

	#[test]
	fn finalized_and_voided_labels_and_void_amount_strikes_the_hold() {
		let cards = cards(vec![
			transfer(1, TransferState::Pending, 2_500, None),
			transfer(2, TransferState::PostPending, 5_000, Some(5_000)),
			// Void keeps the historical hold and posts 0.
			transfer(3, TransferState::VoidPending, 123_456, Some(0)),
		]);

		assert_eq!(cards[0].state_label, Some("Pending"));
		assert_eq!(cards[0].qty, "25");
		assert_eq!(cards[0].voided_qty, None);
		assert_eq!(cards[1].state_label, Some("Finalized"));
		assert_eq!(cards[1].qty, "50");
		assert_eq!(cards[2].state_label, Some("Voided"));
		assert_eq!(cards[2].voided_qty.as_deref(), Some("1,234.56"));
		// Both legs are ours, so All and each account get a reading.
		assert_eq!(cards[2].per_account.len(), 2);
	}

	#[test]
	fn needles_and_sets_match_by_containment() {
		assert_eq!(needle(12), "|12|");
		assert_eq!(id_set([2, 1, 2]), "|1|2|");
		let set = id_set([1, 12]);
		assert!(set.contains(&needle(12)));
		assert!(!set.contains(&needle(2)));
		// All (`""`) is contained in every set.
		assert!(set.contains(""));
	}

	#[test]
	fn days_group_consecutive_cards_with_their_accounts() {
		let now = 1_700_000_000_000_000;
		let mut old = transfer(3, TransferState::Posted, 1, Some(1));
		old.created_at = Timestamp::from_micros_since_unix_epoch(now - 3 * 86_400 * 1_000_000);
		let groups = group_days(
			cards(vec![
				transfer(1, TransferState::Posted, 1, Some(1)),
				transfer(2, TransferState::Posted, 1, Some(1)),
				old,
			]),
			now,
		);
		assert_eq!(groups.len(), 2);
		assert_eq!(groups[0].cards.len(), 2);
		assert_eq!(groups[1].cards.len(), 1);
		assert_eq!(groups[0].set, "|1|2|");
	}
}
