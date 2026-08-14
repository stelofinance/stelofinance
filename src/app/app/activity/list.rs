//! Filter chips + day-grouped cards for GET `/app/activity` and live patches.

use super::present::{AmountSign, TransferCard, account_title, transfer_card};
use crate::module_bindings::MyAccountRow;
use crate::stdb::{ActivitySnapshot, day_heading, format_rel_time};
use std::collections::HashSet;
use topcoat::{
	Result,
	view::{Unescaped, component, view},
};

#[component]
pub async fn activity_body(
	data: ActivitySnapshot,
	selected: Option<u64>,
	now_micros: i64,
) -> Result {
	view! {
		(Unescaped::new_unchecked(activity_body_html(&data, selected, now_micros)))
	}
}

pub fn activity_body_html(
	data: &ActivitySnapshot,
	selected: Option<u64>,
	now_micros: i64,
) -> String {
	let mut out = String::from(r#"<div id="activity-body">"#);
	if data.accounts.is_empty() {
		out.push_str(&empty_html(
			"Nothing here yet.",
			"Create an account to send and receive.",
			false,
		));
		out.push_str("</div>");
		return out;
	}

	push_chips(&mut out, &data.accounts, selected);

	let cards: Vec<TransferCard> = data
		.transfers
		.iter()
		.filter_map(|tr| transfer_card(tr, &data.accounts))
		.collect();

	if cards.is_empty() {
		out.push_str(&empty_html(
			"Nothing here yet.",
			"Send or receive to see activity.",
			true,
		));
		out.push_str("</div>");
		return out;
	}

	push_filter_empty(&mut out, &data.accounts, &cards, selected);
	push_list(&mut out, &cards, selected, now_micros);
	out.push_str("</div>");
	out
}

fn push_chips(out: &mut String, accounts: &[MyAccountRow], selected: Option<u64>) {
	out.push_str(
		r#"<div class="mb-6 flex flex-wrap gap-2" role="tablist" aria-label="Filter by account">"#,
	);
	push_chip(out, "", "All", selected.is_none(), "/app/activity");
	for acc in accounts {
		let id = acc.account_id.to_string();
		let href = format!("/app/activity?account={id}");
		push_chip(
			out,
			&id,
			&account_title(acc),
			selected == Some(acc.account_id),
			&href,
		);
	}
	out.push_str("</div>");
}

fn push_chip(out: &mut String, account_id: &str, label: &str, on: bool, href: &str) {
	let eq = format!("$accountId == '{account_id}'");
	let click =
		format!("$accountId = '{account_id}'; window.history.replaceState(null, '', '{href}')");
	out.push_str(&format!(
		r#"<button type="button" role="tab" class="cursor-pointer rounded-full px-3 py-1.5 text-sm text-neutral-300 hover:bg-neutral-800 hover:text-white" data-class-bg-anakiwa-700="{eq}" data-class-text-white="{eq}" data-attr:aria-pressed="{eq}" data-on:click="{click}" aria-pressed="{}">{}</button>"#,
		if on { "true" } else { "false" },
		escape_html(label),
	));
}

fn push_filter_empty(
	out: &mut String,
	accounts: &[MyAccountRow],
	cards: &[TransferCard],
	selected: Option<u64>,
) {
	let active: HashSet<u64> = cards
		.iter()
		.flat_map(|c| c.involved.iter().copied())
		.collect();
	let empty: Vec<u64> = accounts
		.iter()
		.map(|a| a.account_id)
		.filter(|id| !active.contains(id))
		.collect();
	if empty.is_empty() {
		return;
	}
	let show = empty
		.iter()
		.map(|id| format!("$accountId == '{id}'"))
		.collect::<Vec<_>>()
		.join(" || ");
	let visible = selected.is_some_and(|s| empty.contains(&s));
	out.push_str(&format!(
		r#"<div class="rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center" data-show="{show}"{}>"#,
		hidden(!visible),
	));
	out.push_str(r#"<p class="text-neutral-300">No transfers on this account yet.</p></div>"#);
}

fn push_list(out: &mut String, cards: &[TransferCard], selected: Option<u64>, now_micros: i64) {
	out.push_str(r#"<div id="activity-list" class="flex flex-col">"#);
	let mut last_heading = String::new();
	let mut open = false;
	for (i, card) in cards.iter().enumerate() {
		let heading = day_heading(card.created_micros, now_micros);
		if heading != last_heading {
			if open {
				out.push_str("</div></section>");
			}
			let rest = cards[i..]
				.iter()
				.take_while(|c| day_heading(c.created_micros, now_micros) == heading);
			let ids: HashSet<u64> = rest.flat_map(|c| c.involved.iter().copied()).collect();
			let show = show_expr(&ids.iter().copied().collect::<Vec<_>>());
			let group_visible = selected.is_none() || selected.is_some_and(|s| ids.contains(&s));
			out.push_str(&format!(
				r#"<section class="mb-5 last:mb-0" data-show="{show}"{}>"#,
				hidden(!group_visible),
			));
			out.push_str(&format!(
				r#"<h2 class="mb-3 text-sm font-medium uppercase tracking-wide text-neutral-400">{}</h2>"#,
				escape_html(&heading),
			));
			out.push_str(r#"<div class="flex flex-col gap-3">"#);
			last_heading = heading;
			open = true;
		}
		push_card(out, card, selected, now_micros);
	}
	if open {
		out.push_str("</div></section>");
	}
	out.push_str("</div>");
}

fn push_card(out: &mut String, card: &TransferCard, selected: Option<u64>, now_micros: i64) {
	let show = show_expr(&card.involved);
	let visible = selected.is_none() || selected.is_some_and(|s| card.involved.contains(&s));
	out.push_str(&format!(
		r#"<article class="rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2.5 sm:px-4 sm:py-3" data-transfer-id="{}" data-show="{show}"{}>"#,
		card.id,
		hidden(!visible),
	));

	let all_on = selected.is_none();
	out.push_str(&format!(
		r#"<div data-show="$accountId == ''"{}>"#,
		hidden(!all_on)
	));
	push_line(out, card, &card.all, now_micros);
	out.push_str("</div>");

	for (id, line) in &card.per_account {
		let on = selected == Some(*id);
		out.push_str(&format!(
			r#"<div data-show="$accountId == '{id}'"{}>"#,
			hidden(!on)
		));
		push_line(out, card, line, now_micros);
		out.push_str("</div>");
	}
	out.push_str("</article>");
}

fn push_line(out: &mut String, card: &TransferCard, line: &super::present::Line, now_micros: i64) {
	let qty = match line.sign {
		AmountSign::In => format!("+{}", card.qty),
		AmountSign::Out => format!("−{}", card.qty),
		AmountSign::Flat => card.qty.clone(),
	};
	let qty_class = match line.sign {
		AmountSign::In => "shrink-0 text-xl font-medium tabular-nums text-anakiwa sm:text-2xl",
		AmountSign::Out | AmountSign::Flat => {
			"shrink-0 text-xl font-medium tabular-nums sm:text-2xl"
		}
	};
	out.push_str(r#"<div class="flex items-start justify-between gap-3">"#);
	out.push_str(&format!(
		r#"<p class="min-w-0 truncate text-sm font-medium">{}</p>"#,
		escape_html(&line.title),
	));
	out.push_str(&format!(
		r#"<p class="{qty_class}">{}</p>"#,
		escape_html(&qty),
	));
	out.push_str("</div>");
	out.push_str(r#"<div class="mt-0.5 flex items-baseline justify-between gap-3">"#);
	out.push_str(r#"<p class="min-w-0 truncate text-sm text-neutral-300">"#);
	out.push_str(&escape_html(&line.subtitle));
	if let Some(state) = card.state_label {
		out.push_str(
			r#" <span class="ml-1 rounded-full bg-neutral-800 px-2 py-0.5 text-xs text-neutral-300">"#,
		);
		out.push_str(state);
		out.push_str("</span>");
	}
	out.push_str("</p>");
	out.push_str(&format!(
		r#"<p class="shrink-0 text-xs text-neutral-400">{}</p>"#,
		escape_html(&card.ledger_name),
	));
	out.push_str("</div>");

	let rel = format_rel_time(card.created_micros, now_micros);
	out.push_str(r#"<p class="mt-1 truncate text-xs text-neutral-400">"#);
	out.push_str(&escape_html(&rel));
	if let Some(memo) = &card.memo {
		out.push_str(" · ");
		out.push_str(&escape_html(memo));
	}
	out.push_str("</p>");
}

fn show_expr(ids: &[u64]) -> String {
	let mut out = String::from("$accountId == ''");
	let mut seen = HashSet::new();
	for id in ids {
		if seen.insert(*id) {
			out.push_str(&format!(" || $accountId == '{id}'"));
		}
	}
	out
}

fn hidden(hide: bool) -> &'static str {
	if hide {
		r#" style="display: none""#
	} else {
		""
	}
}

fn empty_html(title: &str, blurb: &str, show_send: bool) -> String {
	let mut out = String::from(
		r#"<div class="rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center">"#,
	);
	out.push_str(&format!(
		r#"<p class="text-neutral-300">{}</p><p class="mt-2 text-sm text-neutral-400">{}</p>"#,
		escape_html(title),
		escape_html(blurb),
	));
	out.push_str(r#"<div class="mt-6 flex flex-wrap justify-center gap-3">"#);
	if show_send {
		out.push_str(
			r#"<a href="/app/transfer" class="inline-flex rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600">Send</a>"#,
		);
	}
	out.push_str(
		r#"<a href="/app/accounts" class="inline-flex rounded-md border border-neutral-600 px-4 py-2 text-sm text-neutral-200 hover:border-neutral-400">Accounts</a>"#,
	);
	out.push_str("</div></div>");
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
