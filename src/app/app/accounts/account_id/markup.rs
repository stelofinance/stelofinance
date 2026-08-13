//! Live `#account-home` fragment for H2.

use crate::module_bindings::{AccountKind, LedgerKind, MemberKind, Role};
use crate::stdb::account::{AccountHomeData, role_rank};
use crate::stdb::format_qty;
use spacetimedb_sdk::Identity;
use topcoat::{
	Result,
	view::{Unescaped, component, view},
};

pub struct HomeChrome {
	pub caller_id: Identity,
	pub caller_username: String,
}

#[component]
pub async fn account_home(data: AccountHomeData, chrome: HomeChrome) -> Result {
	view! {
		(Unescaped::new_unchecked(account_home_html(&data, &chrome)))
	}
}

pub fn account_home_html(data: &AccountHomeData, chrome: &HomeChrome) -> String {
	let mut out = String::from(r#"<div id="account-home" class="mt-4 flex flex-col">"#);
	let Some(acc) = &data.account else {
		out.push_str(
			r#"<p class="text-neutral-300">You no longer have access to this account.</p>
			<a href="/app/accounts" class="mt-4 text-sm text-anakiwa underline-offset-2 hover:underline">Back to Accounts</a></div>"#,
		);
		return out;
	};

	let id = acc.account_id;
	let qty = format_qty(acc.balance, acc.ledger_scale);
	let label = acc
		.label
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty());
	let title = escape_html(label.unwrap_or(acc.ledger_name.as_str()));
	let debit = matches!(acc.kind, AccountKind::Debit);
	let admin_plus = role_rank(acc.role) >= role_rank(Role::Admin);
	let owner = matches!(acc.role, Role::Owner);
	let can_send = debit && role_rank(acc.role) >= role_rank(Role::Write);
	let addr = escape_html(&acc.address);
	let username = escape_html(&chrome.caller_username);

	out.push_str(r#"<div class="flex flex-wrap items-start justify-between gap-3">"#);
	out.push_str("<div>");
	out.push_str(&format!(
		r#"<h1 class="text-2xl font-medium md:text-3xl">{title}</h1>"#
	));
	if label.is_some() {
		out.push_str(&format!(
			r#"<p class="mt-0.5 text-sm text-neutral-400">{}</p>"#,
			escape_html(&acc.ledger_name)
		));
	} else {
		out.push_str(&format!(
			r#"<p class="mt-0.5 text-sm text-neutral-500">{}</p>"#,
			escape_html(ledger_kind_label(acc.ledger_kind))
		));
	}
	out.push_str("</div><div class=\"flex flex-wrap gap-2\">");
	if acc.is_primary {
		out.push_str(
			r#"<span class="rounded-full bg-anakiwa/15 px-2.5 py-0.5 text-xs text-anakiwa">Primary</span>"#,
		);
	}
	if !debit {
		out.push_str(
			r#"<span class="rounded-full bg-neutral-800 px-2.5 py-0.5 text-xs text-neutral-300">Credit</span>"#,
		);
	}
	if !owner {
		out.push_str(&format!(
			r#"<span class="rounded-full bg-neutral-800 px-2.5 py-0.5 text-xs text-neutral-300">Shared · {}</span>"#,
			role_label(acc.role)
		));
	}
	out.push_str("</div></div>");

	out.push_str(&format!(
		r#"<p class="mt-4 text-3xl font-medium tabular-nums sm:text-4xl">{}</p>"#,
		escape_html(&qty)
	));

	out.push_str(
		r#"<div class="mt-3 flex flex-wrap items-center gap-2 text-sm text-neutral-400">"#,
	);
	out.push_str(&format!("<span>#{addr}</span>"));
	out.push_str(&format!(
		r#"<button type="button" class="cursor-pointer rounded-md px-2 py-0.5 text-xs text-neutral-400 hover:bg-neutral-800 hover:text-white" data-address="{addr}" data-on:click="window.navigator.clipboard.writeText(el.dataset.address); $copiedId = 1" data-on:click__delay.2000ms="$copiedId = 0" data-text="$copiedId == 1 ? 'Copied' : 'Copy'" aria-label="Copy address {addr}">Copy</button>"#
	));
	out.push_str("</div>");

	if debit {
		out.push_str(r#"<p class="mt-3 text-sm text-neutral-400">"#);
		if acc.is_primary {
			out.push_str(&format!("People can send to @{username}."));
		} else {
			out.push_str(&format!("Not primary. Sends to @{username} go elsewhere."));
		}
		out.push_str("</p>");
	}

	if owner && debit {
		out.push_str(r#"<div class="mt-3">"#);
		if acc.is_primary {
			out.push_str(&format!(
				r#"<button type="button" class="cursor-pointer rounded-md border border-neutral-700 px-3 py-1.5 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white" data-on:click="@post('/app/accounts/{id}/primary')" data-indicator="savingPrimary">Turn off primary</button>"#
			));
		} else if data.has_other_primary {
			out.push_str(
				r#"<p class="text-sm text-neutral-500">You already have a primary account for this asset.</p>"#,
			);
		} else {
			out.push_str(&format!(
				r#"<button type="button" class="cursor-pointer rounded-md bg-anakiwa-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-anakiwa-600" data-on:click="@post('/app/accounts/{id}/primary')" data-indicator="savingPrimary">Make primary</button>"#
			));
		}
		out.push_str("</div>");
	}

	if can_send {
		out.push_str(
			r#"<div class="mt-5"><a href="/app/transfer" class="inline-flex cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600">Send</a></div>"#,
		);
	}

	push_people(
		&mut out,
		id,
		acc.role,
		&data.members,
		chrome,
		admin_plus,
		owner,
	);
	out.push_str("</div>");
	out
}

fn push_people(
	out: &mut String,
	account_id: u64,
	caller_role: Role,
	members: &[crate::module_bindings::MyAccountMemberRow],
	chrome: &HomeChrome,
	admin_plus: bool,
	caller_is_owner: bool,
) {
	out.push_str(
		r#"<section class="mt-10"><h2 class="text-sm font-medium uppercase tracking-wide text-neutral-500">People</h2>"#,
	);
	out.push_str(r#"<div class="mt-3 flex flex-col gap-2">"#);
	for m in members {
		let self_row = m.member_id == chrome.caller_id;
		let target_owner = matches!(m.role, Role::Owner);
		let name = escape_html(&m.name);
		out.push_str(
			r#"<div class="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2.5">"#,
		);
		out.push_str(r#"<div class="min-w-0">"#);
		out.push_str(&format!(
			r#"<p class="truncate text-sm text-neutral-100">{name}"#
		));
		if self_row {
			out.push_str(r#" <span class="text-neutral-500">(you)</span>"#);
		}
		if matches!(m.kind, MemberKind::App) {
			out.push_str(
				r#" <span class="ml-1 rounded-full bg-neutral-800 px-2 py-0.5 text-xs text-neutral-400">App</span>"#,
			);
		}
		out.push_str("</p></div>");

		out.push_str(r#"<div class="flex flex-wrap items-center gap-2">"#);
		if admin_plus && !self_row && !target_owner {
			push_role_select(out, account_id, m.member_id, m.role, caller_is_owner);
			let hex = m.member_id.to_hex();
			out.push_str(&format!(
				r#"<button type="button" class="cursor-pointer text-sm text-red-400 hover:text-red-300" data-on:click="$revokeId = '{hex}'; @post('/app/accounts/{account_id}/revoke')">Remove</button>"#
			));
		} else {
			out.push_str(&format!(
				r#"<span class="text-sm text-neutral-400">{}</span>"#,
				role_label(m.role)
			));
		}
		if self_row && !matches!(caller_role, Role::Owner) {
			out.push_str(&format!(
				r#"<button type="button" class="cursor-pointer text-sm text-neutral-400 hover:text-white" data-on:click="@post('/app/accounts/{account_id}/leave')">Leave</button>"#
			));
		}
		out.push_str("</div></div>");
	}
	out.push_str("</div></section>");
}

fn push_role_select(
	out: &mut String,
	account_id: u64,
	member_id: Identity,
	current: Role,
	caller_is_owner: bool,
) {
	let hex = member_id.to_hex();
	out.push_str(&format!(
		r#"<select class="cursor-pointer rounded-md border border-neutral-800 bg-neutral-900 px-2 py-1 text-sm" data-on:change="$editMemberId = '{hex}'; $editRole = el.value; @post('/app/accounts/{account_id}/members')">"#
	));
	for (value, role) in [
		("read", Role::Read),
		("write", Role::Write),
		("admin", Role::Admin),
	] {
		let sel = if role == current { " selected" } else { "" };
		out.push_str(&format!(
			r#"<option value="{value}"{sel}>{}</option>"#,
			role_label(role)
		));
	}
	if caller_is_owner {
		let sel = if matches!(current, Role::Owner) {
			" selected"
		} else {
			""
		};
		out.push_str(&format!(r#"<option value="owner"{sel}>Owner</option>"#));
	}
	out.push_str("</select>");
}

fn ledger_kind_label(kind: LedgerKind) -> &'static str {
	match kind {
		LedgerKind::Physical => "In-game item",
		LedgerKind::Digital => "Digital",
		LedgerKind::Derivation => "Derivation",
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
