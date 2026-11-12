//! Grouped portfolio markup for GET `/app/accounts` and live `#accounts-list` patches.

use topcoat::context::Cx;
use crate::domain::accounts::{LedgerGroup, group_by_ledger};
use crate::ui::text::{escape_html, ledger_kind_label, role_label};
use crate::module_bindings::{AccountKind, MyAccountRow, Role};
use crate::domain::format_qty;
use topcoat::{
	Result,
	view::{Unescaped, View, component, view},
};

/// SSR wrapper so the live stream can reuse the same fragment.
#[component]
pub async fn accounts_list(cx: &Cx, rows: Vec<MyAccountRow>) -> Result<impl View> {
	Ok(view! { (Unescaped::new_unchecked(accounts_list_html(&rows))) })
}

/// Full `#accounts-list` fragment (empty state + grouped cards).
pub fn accounts_list_html(rows: &[MyAccountRow]) -> String {
	let mut out = String::from(r#"<div id="accounts-list" class="flex flex-col">"#);
	if rows.is_empty() {
		out.push_str(empty_state_html());
	} else {
		let debit: Vec<&MyAccountRow> = rows
			.iter()
			.filter(|a| matches!(a.kind, AccountKind::Debit))
			.collect();
		let credit: Vec<&MyAccountRow> = rows
			.iter()
			.filter(|a| matches!(a.kind, AccountKind::Credit))
			.collect();

		for group in group_by_ledger(debit.iter().copied()) {
			push_ledger_group(&mut out, &group);
		}
		if !credit.is_empty() {
			out.push_str(
				r#"<h2 class="mt-4 mb-3 text-sm font-medium uppercase tracking-wide text-neutral-400">Issuer accounts</h2>"#,
			);
			for group in group_by_ledger(credit.iter().copied()) {
				push_ledger_group(&mut out, &group);
			}
		}
	}
	out.push_str("</div>");
	out
}

fn push_ledger_group(out: &mut String, group: &LedgerGroup<'_>) {
	out.push_str(r#"<section class="mb-5 last:mb-0">"#);
	out.push_str(r#"<div class="mb-3 flex flex-wrap items-baseline gap-x-2 gap-y-0.5">"#);
	out.push_str(r#"<h2 class="text-sm font-medium uppercase tracking-wide text-neutral-300">"#);
	out.push_str(&escape_html(group.ledger_name));
	out.push_str("</h2>");
	out.push_str(r#"<span class="text-xs text-neutral-400">"#);
	out.push_str(escape_html(ledger_kind_label(group.ledger_kind)).as_str());
	out.push_str("</span></div>");
	out.push_str(r#"<div class="flex flex-col gap-3">"#);
	for acc in &group.accounts {
		push_account_card(out, acc);
	}
	out.push_str("</div></section>");
}

fn push_account_card(out: &mut String, acc: &MyAccountRow) {
	let qty = format_qty(acc.balance, acc.ledger_scale);
	let href = format!("/app/accounts/{}", acc.account_id);
	let addr = escape_html(&acc.address);
	let label = acc
		.label
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty())
		.map(escape_html);

	let aria = match &label {
		Some(l) => format!("{l}, {qty}"),
		None => format!("#{}, {qty}", escape_html(&acc.address)),
	};

	out.push_str(
		r#"<article class="flex overflow-hidden rounded-lg border border-neutral-800 bg-neutral-950 transition-colors hover:border-neutral-600">"#,
	);
	out.push_str(&format!(
		r#"<a href="{href}" class="min-w-0 flex-1 px-3 py-2.5 focus:outline-none focus-visible:ring-2 focus-visible:ring-anakiwa/50 sm:px-4 sm:py-3" aria-label="{}">"#,
		escape_html(&aria),
	));

	out.push_str(r#"<div class="flex items-start justify-between gap-3">"#);
	out.push_str(&format!(
		r#"<p class="text-xl font-medium tabular-nums sm:text-2xl">{}</p>"#,
		escape_html(&qty),
	));
	push_card_badge(out, acc);
	out.push_str("</div>");

	if let Some(label) = &label {
		out.push_str(&format!(
			r#"<p class="mt-1 truncate text-sm text-neutral-200">{label}</p>"#
		));
	}

	out.push_str(&format!(
		r#"<p class="mt-1 truncate text-sm text-neutral-300">#{}{}</p>"#,
		addr,
		meta_suffix(acc),
	));
	out.push_str("</a>");

	out.push_str(&format!(
		r#"<button type="button" class="shrink-0 cursor-pointer self-stretch border-l border-neutral-800 px-3 text-xs text-neutral-300 hover:bg-neutral-900 hover:text-white" data-address="{addr}" data-on:click="window.navigator.clipboard.writeText(el.dataset.address); $copiedId = {id}" data-on:click__delay.2000ms="$copiedId = 0" data-text="$copiedId == {id} ? 'Copied' : 'Copy'" aria-label="Copy address {addr}">Copy</button>"#,
		id = acc.account_id,
	));
	out.push_str("</article>");
}

fn push_card_badge(out: &mut String, acc: &MyAccountRow) {
	if acc.is_primary {
		out.push_str(
			r#"<span class="shrink-0 rounded-full bg-anakiwa/15 px-2.5 py-0.5 text-xs text-anakiwa">Primary</span>"#,
		);
	} else if matches!(acc.kind, AccountKind::Credit) {
		out.push_str(
			r#"<span class="shrink-0 rounded-full bg-neutral-800 px-2.5 py-0.5 text-xs text-neutral-300">Credit</span>"#,
		);
	}
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
		s.push_str(&escape_html(owner));
	}
	s
}

fn empty_state_html() -> &'static str {
	r#"<div class="rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center" data-show="!$creatingAcc">
		<p class="text-neutral-300">You don't have any accounts yet.</p>
		<p class="mt-2 text-sm text-neutral-400">Create one to hold assets and receive from other players.</p>
		<button type="button" class="mt-6 rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600" data-on:click="$creatingAcc = true; $createError = ''">Create an account</button>
	</div>"#
}



