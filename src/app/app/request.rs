//! `GET/POST /app/request` — H4 pay a payment-request link.

use super::transfer::markup::from_panel;
use crate::auth::require_user;
use crate::module_bindings::{AccountSearchHit, MyAccountRow};
use crate::stdb::{
	StdbError, acquire_user_db, create_user_transfer, fetch_accounts_page, format_qty,
	lookup_account, map_transfer_error, new_idempotency_key, pay_from_accounts, pick_sendable,
};
use serde::{Deserialize, Serialize};
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchSignals, Signals},
	router::{error::internal_server_error, page, query_params, route},
	view::{Unescaped, component, view},
};

const MAX_MEMO_LEN: usize = 32;

#[query_params]
struct PayQuery {
	ledgerid: Option<String>,
	recipientid: Option<String>,
	amount: Option<String>,
	memo: Option<String>,
}

#[derive(Debug)]
struct PaySpec {
	ledger_id: u64,
	recipient_id: u64,
	amount: u64,
	memo: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PayQueryError {
	Incomplete,
	Invalid,
}

/// `GET /app/request` — confirm a query-prefilled payment.
#[page]
async fn page(cx: &Cx) -> Result {
	let _user = require_user(cx).await?;
	let raw = query_params::<PayQuery>(cx).ok();
	let ledgerid = raw.and_then(|q| q.ledgerid.clone());
	let recipientid = raw.and_then(|q| q.recipientid.clone());
	let amount = raw.and_then(|q| q.amount.clone());
	let memo = raw.and_then(|q| q.memo.clone());
	let spec = match parse_pay_query(
		ledgerid.as_deref(),
		recipientid.as_deref(),
		amount.as_deref(),
		memo.as_deref(),
	) {
		Ok(s) => s,
		Err(_) => {
			return view! {
				invalid_link()
			};
		}
	};

	let conn = acquire_user_db(cx).await?;
	let data = fetch_accounts_page(conn.get())
		.await
		.map_err(|e| internal_server_error(StdbError(e)))?;
	let Some(ledger) = data
		.ledgers
		.iter()
		.find(|l| l.id == spec.ledger_id)
		.cloned()
	else {
		return view! {
			invalid_link()
		};
	};

	let recipient = match lookup_account(conn.get(), spec.recipient_id).await {
		Ok(hit) if hit.ledger_id == spec.ledger_id => hit,
		Ok(_) | Err(_) => {
			return view! {
				invalid_link()
			};
		}
	};

	let accounts = pay_from_accounts(&data.accounts, spec.ledger_id, spec.recipient_id);
	let can_pick = accounts.len() > 1;
	let selected = pick_sendable(&accounts, None);
	let selected_id = selected.map(|a| a.account_id).unwrap_or(0);
	let from_id = if selected_id == 0 {
		String::new()
	} else {
		selected_id.to_string()
	};
	let key = new_idempotency_key();
	let memo_js = spec.memo.as_deref().unwrap_or("");
	let signals = format!(
		"{{fromId:'{from_id}',fromLedgerId:'{}',pickingFrom:false,recipientId:'{}',amount:'{}',memo:{},idempotencyKey:'{key}',sendError:'',sent:false}}",
		spec.ledger_id,
		spec.recipient_id,
		spec.amount,
		js_single(memo_js),
	);

	let invoice = Invoice {
		qty: format_qty(spec.amount, ledger.scale),
		ledger_name: ledger.name.clone(),
		recipient_name: recipient_name(&recipient),
		recipient_addr: recipient_addr(&recipient),
		memo: spec.memo.clone(),
	};

	view! {
		<main
			id="page-content"
			class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
			data-signals=(signals)
		>
			<h1 class="mb-6 text-2xl font-medium md:text-3xl">"Pay"</h1>
			invoice_card(invoice: invoice.clone())
			if accounts.is_empty() {
				no_account(ledger_name: ledger.name)
			} else {
				pay_form(
					accounts: accounts,
					can_pick: can_pick,
					selected_id: selected_id,
					invoice: invoice,
					amount: spec.amount,
				)
			}
		</main>
	}
}

#[derive(Clone)]
struct Invoice {
	qty: String,
	ledger_name: String,
	recipient_name: String,
	recipient_addr: String,
	memo: Option<String>,
}

#[component]
async fn invalid_link() -> Result {
	view! {
		<main class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10">
			<h1 class="mb-6 text-2xl font-medium md:text-3xl">"Pay"</h1>
			<div class="rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center">
				<p class="text-neutral-300">"This payment link isn't valid."</p>
				<p class="mt-2 text-sm text-neutral-400">"Ask the sender for a new one."</p>
				<a
					href="/app/accounts"
					class="mt-6 inline-flex rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
				>
					"Accounts"
				</a>
			</div>
		</main>
	}
}

#[component]
async fn no_account(#[into] ledger_name: String) -> Result {
	view! {
		<div class="mt-6 rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center">
			<p class="text-neutral-300">
				"You need a writable "
				(ledger_name)
				" account to pay this."
			</p>
			<p class="mt-2 text-sm text-neutral-400">
				"Create a debit account, or ask an owner to grant you Write."
			</p>
			<a
				href="/app/accounts"
				class="mt-6 inline-flex rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
			>
				"Accounts"
			</a>
		</div>
	}
}

#[component]
async fn invoice_card(invoice: Invoice) -> Result {
	view! {
		(Unescaped::new_unchecked(invoice_html(&invoice)))
	}
}

fn invoice_html(invoice: &Invoice) -> String {
	let mut out = String::from(
		r#"<article class="rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-3 sm:px-4 sm:py-4">"#,
	);
	out.push_str(r#"<div class="flex items-baseline justify-between gap-3">"#);
	out.push_str(&format!(
		r#"<p class="text-2xl font-medium tabular-nums sm:text-3xl">{}</p>"#,
		escape_html(&invoice.qty),
	));
	out.push_str(&format!(
		r#"<p class="shrink-0 text-sm text-neutral-300">{}</p>"#,
		escape_html(&invoice.ledger_name),
	));
	out.push_str("</div>");
	out.push_str(r#"<p class="mt-2 truncate text-sm text-neutral-300">to "#);
	out.push_str(&escape_html(&invoice.recipient_name));
	if !invoice.recipient_addr.is_empty() {
		out.push_str(" · ");
		out.push_str(&escape_html(&invoice.recipient_addr));
	}
	out.push_str("</p>");
	if let Some(memo) = &invoice.memo {
		out.push_str(&format!(
			r#"<p class="mt-1 truncate text-sm text-neutral-400">{}</p>"#,
			escape_html(memo),
		));
	}
	out.push_str("</article>");
	out
}

#[component]
async fn pay_form(
	accounts: Vec<MyAccountRow>,
	can_pick: bool,
	selected_id: u64,
	invoice: Invoice,
	amount: u64,
) -> Result {
	let short = short_expr(&accounts, amount);
	let disabled = if short.is_empty() {
		"$sending || !$fromId".to_owned()
	} else {
		format!("$sending || !$fromId || ({short})")
	};
	let cta = format!("Pay {} {}", invoice.qty, invoice.ledger_name);
	let sent_to = invoice.recipient_name.clone();
	let sent_addr = invoice.recipient_addr.clone();
	let sent_qty = format!("{} {}", invoice.qty, invoice.ledger_name);

	view! {
		<div class="mt-6" data-show="!$sent">
			from_panel(accounts: accounts.clone(), can_pick: can_pick, selected_id: selected_id)
			short_hints(accounts: accounts, amount: amount, selected_id: selected_id)
			<button
				type="button"
				class="mt-6 w-full cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
				data-on:click="@post('/app/request')"
				data-indicator="sending"
				data-attr-disabled=(disabled)
			>
				(cta)
			</button>
			<p
				class="mt-3 text-sm text-red-400"
				data-show="$sendError"
				data-text="$sendError"
			></p>
		</div>
		<div data-show="$sent" style="display: none">
			<div class="mt-6 rounded-lg border border-neutral-800 bg-neutral-950 px-4 py-6 sm:px-5">
				<h2 class="text-xl font-medium">
					"Sent "
					(sent_qty)
				</h2>
				<p class="mt-1 text-sm text-neutral-300">
					"to "
					(sent_to)
					if !sent_addr.is_empty() {
						" · "
						(sent_addr)
					}
				</p>
				<div class="mt-5 flex flex-wrap gap-3">
					<a
						href="/app/activity"
						class="inline-flex rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
					>
						"View activity"
					</a>
					<a
						href="/app/accounts"
						class="inline-flex rounded-md border border-neutral-600 px-4 py-2 text-sm text-neutral-200 hover:border-neutral-400"
					>
						"Accounts"
					</a>
				</div>
			</div>
		</div>
	}
}

#[component]
async fn short_hints(accounts: Vec<MyAccountRow>, amount: u64, selected_id: u64) -> Result {
	view! {
		for acc in accounts.iter().filter(|a| a.balance < amount) {
			short_hint(acc: acc, amount: amount, selected: acc.account_id == selected_id)
		}
	}
}

#[component]
async fn short_hint(acc: &MyAccountRow, amount: u64, selected: bool) -> Result {
	let _ = amount;
	let show = format!("$fromId == '{}'", acc.account_id);
	if selected {
		view! {
			<p class="mt-2 text-sm text-red-400" data-show=(show)>
				"Not enough available on this account."
			</p>
		}
	} else {
		view! {
			<p
				class="mt-2 text-sm text-red-400"
				data-show=(show)
				style="display: none"
			>
				"Not enough available on this account."
			</p>
		}
	}
}

/// `POST /app/request` — command only. Success flips `$sent`.
#[route(POST)]
async fn create(cx: &Cx, Signals(form): Signals<PaySignals>) -> Result<PatchSignals> {
	let _user = require_user(cx).await?;
	let Ok(from_id) = form.from_id.trim().parse::<u64>() else {
		return pay_err("Choose an account to pay from.", &form);
	};
	let Ok(recipient_id) = form.recipient_id.trim().parse::<u64>() else {
		return pay_err("This payment link isn't valid.", &form);
	};
	let Ok(amount) = form.amount.trim().parse::<u64>() else {
		return pay_err("This payment link isn't valid.", &form);
	};
	if amount == 0 {
		return pay_err("This payment link isn't valid.", &form);
	}
	if form.idempotency_key.trim().is_empty() {
		return pay_err("Refresh the page and try again.", &form);
	}

	let conn = acquire_user_db(cx).await?;
	let data = match fetch_accounts_page(conn.get()).await {
		Ok(d) => d,
		Err(e) => return pay_err(&e, &form),
	};
	let recipient = match lookup_account(conn.get(), recipient_id).await {
		Ok(hit) => hit,
		Err(_) => return pay_err("This payment link isn't valid.", &form),
	};
	let from_accounts = pay_from_accounts(&data.accounts, recipient.ledger_id, recipient_id);
	let Some(from) = from_accounts.iter().find(|a| a.account_id == from_id) else {
		return pay_err("You can't pay from that account.", &form);
	};
	if from.ledger_id != recipient.ledger_id {
		return pay_err("This payment link isn't valid.", &form);
	}
	if amount > from.balance {
		return pay_err("Not enough available balance.", &form);
	}

	let memo = {
		let t = form.memo.trim();
		if t.is_empty() {
			None
		} else if t.len() > MAX_MEMO_LEN {
			return pay_err("This payment link isn't valid.", &form);
		} else {
			Some(t.to_owned())
		}
	};

	match create_user_transfer(
		conn.get(),
		from_id,
		recipient_id,
		amount,
		memo,
		form.idempotency_key.trim().to_owned(),
	)
	.await
	{
		Ok(()) => PatchSignals::json(&PayPatch {
			send_error: String::new(),
			sent: true,
		}),
		Err(e) => pay_err(&map_transfer_error(&e), &form),
	}
}

#[derive(Debug, Deserialize)]
struct PaySignals {
	#[serde(default, rename = "fromId")]
	from_id: String,
	#[serde(default, rename = "recipientId")]
	recipient_id: String,
	#[serde(default)]
	amount: String,
	#[serde(default)]
	memo: String,
	#[serde(default, rename = "idempotencyKey")]
	idempotency_key: String,
}

#[derive(Serialize)]
struct PayPatch {
	#[serde(rename = "sendError")]
	send_error: String,
	sent: bool,
}

fn pay_err(error: &str, _form: &PaySignals) -> Result<PatchSignals> {
	PatchSignals::json(&PayPatch {
		send_error: error.to_owned(),
		sent: false,
	})
}

fn parse_pay_query(
	ledgerid: Option<&str>,
	recipientid: Option<&str>,
	amount: Option<&str>,
	memo: Option<&str>,
) -> Result<PaySpec, PayQueryError> {
	let ledgerid = ledgerid.map(str::trim).filter(|s| !s.is_empty());
	let recipientid = recipientid.map(str::trim).filter(|s| !s.is_empty());
	let amount = amount.map(str::trim).filter(|s| !s.is_empty());
	if ledgerid.is_none() || recipientid.is_none() || amount.is_none() {
		return Err(PayQueryError::Incomplete);
	}
	let ledger_id = ledgerid
		.unwrap()
		.parse::<u64>()
		.map_err(|_| PayQueryError::Invalid)?;
	let recipient_id = recipientid
		.unwrap()
		.parse::<u64>()
		.map_err(|_| PayQueryError::Invalid)?;
	let amount = amount
		.unwrap()
		.parse::<u64>()
		.map_err(|_| PayQueryError::Invalid)?;
	if amount == 0 {
		return Err(PayQueryError::Invalid);
	}
	let memo = match memo {
		None => None,
		Some(raw) => {
			let t = raw.trim();
			if t.is_empty() {
				None
			} else if t.len() > MAX_MEMO_LEN {
				return Err(PayQueryError::Invalid);
			} else {
				Some(t.to_owned())
			}
		}
	};
	Ok(PaySpec {
		ledger_id,
		recipient_id,
		amount,
		memo,
	})
}

fn short_expr(accounts: &[MyAccountRow], amount: u64) -> String {
	accounts
		.iter()
		.filter(|a| a.balance < amount)
		.map(|a| format!("$fromId == '{}'", a.account_id))
		.collect::<Vec<_>>()
		.join(" || ")
}

fn recipient_name(hit: &AccountSearchHit) -> String {
	hit.primary_username
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty())
		.map(|u| format!("@{u}"))
		.unwrap_or_else(|| format!("#{}", hit.address))
}

fn recipient_addr(hit: &AccountSearchHit) -> String {
	if hit
		.primary_username
		.as_deref()
		.map(str::trim)
		.is_some_and(|s| !s.is_empty())
	{
		format!("#{}", hit.address)
	} else {
		String::new()
	}
}

fn js_single(s: &str) -> String {
	let mut out = String::from('\'');
	for c in s.chars() {
		match c {
			'\\' => out.push_str("\\\\"),
			'\'' => out.push_str("\\'"),
			'\n' | '\r' => {}
			c => out.push(c),
		}
	}
	out.push('\'');
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

#[cfg(test)]
mod tests {
	use super::{PayQueryError, parse_pay_query};

	#[test]
	fn missing_is_incomplete() {
		assert_eq!(
			parse_pay_query(None, Some("1"), Some("5"), None).unwrap_err(),
			PayQueryError::Incomplete
		);
	}

	#[test]
	fn zero_amount_invalid() {
		assert_eq!(
			parse_pay_query(Some("1"), Some("2"), Some("0"), None).unwrap_err(),
			PayQueryError::Invalid
		);
	}

	#[test]
	fn long_memo_invalid() {
		let memo = "x".repeat(33);
		assert_eq!(
			parse_pay_query(Some("1"), Some("2"), Some("10"), Some(&memo)).unwrap_err(),
			PayQueryError::Invalid
		);
	}

	#[test]
	fn happy_path() {
		let spec =
			parse_pay_query(Some("1"), Some("42"), Some("5000"), Some(" Invoice #123 ")).unwrap();
		assert_eq!(spec.ledger_id, 1);
		assert_eq!(spec.recipient_id, 42);
		assert_eq!(spec.amount, 5000);
		assert_eq!(spec.memo.as_deref(), Some("Invoice #123"));
	}
}
