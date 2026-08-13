//! Send-page fragments: from-account cards / picker + recipient results.

use crate::module_bindings::{MyAccountRow, Role};
use crate::stdb::DirectoryHit;
use crate::stdb::format_qty;
use topcoat::{
	Result,
	view::{Unescaped, component, view},
};

#[component]
pub async fn from_panel(accounts: Vec<MyAccountRow>, can_pick: bool, selected_id: u64) -> Result {
	view! {
		(Unescaped::new_unchecked(from_panel_html(&accounts, can_pick, selected_id)))
	}
}

pub fn from_panel_html(accounts: &[MyAccountRow], can_pick: bool, selected_id: u64) -> String {
	let mut out = String::from(r#"<div id="from-panel">"#);
	out.push_str(
		r#"<p class="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-500">From</p>"#,
	);
	for acc in accounts {
		push_selected_card(&mut out, acc, can_pick, selected_id);
	}
	if can_pick {
		push_picker(&mut out, accounts);
	}
	out.push_str("</div>");
	out
}

fn push_selected_card(out: &mut String, acc: &MyAccountRow, can_pick: bool, selected_id: u64) {
	let id = acc.account_id;
	let show = format!("$fromId == '{id}' && !$pickingFrom");
	let hidden = if acc.account_id == selected_id {
		""
	} else {
		r#" style="display: none""#
	};
	out.push_str(&format!(
		r#"<article class="rounded-lg border border-neutral-800 bg-neutral-950" data-show="{show}"{hidden}>"#
	));
	if can_pick {
		out.push_str(
			r#"<button type="button" class="flex w-full cursor-pointer items-start justify-between gap-3 px-3 py-2.5 text-left hover:bg-neutral-900 sm:px-4 sm:py-3" data-on:click="$pickingFrom = true" aria-label="Change sending account">"#,
		);
		push_from_body(out, acc, true);
		out.push_str("</button>");
	} else {
		out.push_str(
			r#"<div class="flex items-start justify-between gap-3 px-3 py-2.5 sm:px-4 sm:py-3">"#,
		);
		push_from_body(out, acc, true);
		out.push_str("</div>");
	}
	out.push_str("</article>");
}

fn push_from_body(out: &mut String, acc: &MyAccountRow, show_ledger: bool) {
	let qty = format_qty(acc.balance, acc.ledger_scale);
	let title = from_title(acc);
	out.push_str(r#"<div class="min-w-0">"#);
	out.push_str(&format!(
		r#"<p class="truncate text-lg font-medium">{title}"#
	));
	push_pills(out, acc);
	out.push_str("</p>");
	out.push_str(&format!(
		r#"<p class="mt-1 truncate text-sm text-neutral-400">{}</p>"#,
		from_subtitle(acc, show_ledger)
	));
	out.push_str("</div>");
	out.push_str(&format!(
		r#"<div class="shrink-0 text-right"><p class="text-2xl font-medium tabular-nums sm:text-3xl">{}</p><p class="mt-0.5 text-xs text-neutral-500">available</p></div>"#,
		escape_html(&qty)
	));
}

fn push_picker(out: &mut String, accounts: &[MyAccountRow]) {
	out.push_str(
		r#"<div class="overflow-hidden rounded-lg border border-neutral-800 bg-neutral-950" data-show="$pickingFrom" style="display: none">"#,
	);
	for group in groups(accounts) {
		out.push_str(&format!(
			r#"<p class="px-3 pt-3 pb-1 text-xs font-medium uppercase tracking-wide text-neutral-500">{}</p>"#,
			escape_html(group.ledger_name)
		));
		for acc in group.accounts {
			let id = acc.account_id;
			let ledger = acc.ledger_id;
			let click = format!(
				"if ($fromLedgerId != '{ledger}') {{ $recipientId = ''; $recipientName = ''; $recipientAddr = ''; $recipientLabel = ''; $recipientSearch = ''; }} $fromId = '{id}'; $fromLedgerId = '{ledger}'; $pickingFrom = false; $sendError = ''"
			);
			let on = format!("$fromId == '{id}'");
			out.push_str(&format!(
				r#"<button type="button" class="flex w-full cursor-pointer items-start justify-between gap-3 border-l-2 border-transparent px-3 py-2.5 text-left hover:bg-neutral-900 sm:px-4" data-on:click="{click}" data-class-border-anakiwa="{on}" data-class-bg-neutral-900="{on}">"#
			));
			let title = from_title(acc);
			out.push_str(r#"<div class="min-w-0">"#);
			out.push_str(&format!(
				r#"<p class="truncate text-sm font-medium">{title}"#
			));
			push_pills(out, acc);
			out.push_str("</p>");
			out.push_str(&format!(
				r#"<p class="mt-0.5 truncate text-xs text-neutral-400">{}</p></div>"#,
				from_subtitle(acc, false)
			));
			out.push_str(&format!(
				r#"<p class="shrink-0 text-lg font-medium tabular-nums">{}</p></button>"#,
				escape_html(&format_qty(acc.balance, acc.ledger_scale))
			));
		}
	}
	out.push_str("</div>");
}

struct LedgerGroup<'a> {
	ledger_name: &'a str,
	accounts: Vec<&'a MyAccountRow>,
}

fn groups<'a>(rows: &'a [MyAccountRow]) -> Vec<LedgerGroup<'a>> {
	let mut groups: Vec<LedgerGroup<'a>> = Vec::new();
	for acc in rows {
		if let Some(g) = groups
			.iter_mut()
			.find(|g| g.accounts[0].ledger_id == acc.ledger_id)
		{
			g.accounts.push(acc);
		} else {
			groups.push(LedgerGroup {
				ledger_name: acc.ledger_name.as_str(),
				accounts: vec![acc],
			});
		}
	}
	groups
}

fn from_title(acc: &MyAccountRow) -> String {
	if let Some(label) = acc
		.label
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty())
	{
		escape_html(label)
	} else {
		format!("#{}", escape_html(&acc.address))
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
		parts.push(format!("#{}", escape_html(&acc.address)));
	}
	if matches!(acc.role, Role::Owner) {
		parts.push("Yours".to_owned());
	} else {
		parts.push(format!("Shared · {}", role_label(acc.role)));
	}
	if show_ledger {
		parts.push(escape_html(&acc.ledger_name));
	}
	parts.join(" · ")
}

fn push_pills(out: &mut String, acc: &MyAccountRow) {
	if acc.is_primary {
		out.push_str(
			r#" <span class="ml-1 rounded-full bg-anakiwa/15 px-2 py-0.5 text-xs font-normal text-anakiwa">Primary</span>"#,
		);
	}
}

fn role_label(role: Role) -> &'static str {
	match role {
		Role::Read => "Read",
		Role::Write => "Write",
		Role::Admin => "Admin",
		Role::Owner => "Owner",
	}
}

pub fn recipient_results_html(hits: &[DirectoryHit], searched: bool) -> String {
	if !searched {
		return String::from(r#"<div id="recipient-results"></div>"#);
	}
	let mut out = String::from(
		r#"<div id="recipient-results" class="mt-1 overflow-hidden rounded-md border border-neutral-800 bg-neutral-900" data-show="!$recipientId && $recipientSearch != ''">"#,
	);
	if hits.is_empty() {
		out.push_str(r#"<p class="px-3 py-2 text-sm text-neutral-500">No accounts match.</p>"#);
	} else {
		for hit in hits {
			push_hit(&mut out, hit);
		}
	}
	out.push_str("</div>");
	out
}

fn push_hit(out: &mut String, hit: &DirectoryHit) {
	let (name, addr) = hit_identity(hit);
	let label = hit.own_label.as_deref().unwrap_or("");
	let click = format!(
		"$recipientId = '{}'; $recipientName = '{}'; $recipientAddr = '{}'; $recipientLabel = '{}'; $recipientSearch = ''; $sendError = ''",
		hit.account_id,
		js_single(&name),
		js_single(&addr),
		js_single(label),
	);
	out.push_str(&format!(
		r#"<button type="button" class="block w-full cursor-pointer px-3 py-2 text-left hover:bg-neutral-800" data-on:click="{click}">"#
	));
	if !label.is_empty() {
		out.push_str(&format!(
			r#"<p class="truncate text-sm text-neutral-100">{}</p>"#,
			escape_html(label)
		));
	}
	out.push_str(&format!(
		r#"<p class="truncate text-sm text-neutral-100">{}</p>"#,
		escape_html(&name)
	));
	if !addr.is_empty() {
		out.push_str(&format!(
			r#"<p class="truncate text-xs text-neutral-500">{}</p>"#,
			escape_html(&addr)
		));
	}
	out.push_str("</button>");
}

/// Primary line + optional `#address` secondary (when the primary is `@user`).
pub fn hit_identity(hit: &DirectoryHit) -> (String, String) {
	match hit
		.primary_username
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty())
	{
		Some(user) => (format!("@{user}"), format!("#{}", hit.address)),
		None => (format!("#{}", hit.address), String::new()),
	}
}

fn js_single(s: &str) -> String {
	let mut out = String::new();
	for c in s.chars() {
		match c {
			'\\' => out.push_str("\\\\"),
			'\'' => out.push_str("\\'"),
			'\n' => out.push_str("\\n"),
			c => out.push(c),
		}
	}
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
