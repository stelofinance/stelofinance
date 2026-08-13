//! `GET/POST /app/transfer` — H3a send. Recipient search: `recipients`.

mod markup;
mod recipients;

use crate::auth::require_user;
use crate::module_bindings::MyAccountRow;
use crate::stdb::{
	StdbError, acquire_user_db, create_user_transfer, fetch_accounts_page, map_transfer_error,
	new_idempotency_key, parse_qty, sendable_accounts,
};
use markup::from_panel;
use serde::{Deserialize, Serialize};
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchSignals, Signals},
	router::{error::internal_server_error, page, query_params, route},
	view::{component, view},
};

#[query_params]
struct SendQuery {
	from: Option<String>,
}

/// `GET /app/transfer` — from-account, recipient, amount, memo.
#[page]
async fn page(cx: &Cx) -> Result {
	let _user = require_user(cx).await?;
	let requested = query_params::<SendQuery>(cx)
		.ok()
		.and_then(|q| q.from.clone())
		.and_then(|s| s.parse::<u64>().ok());

	let conn = acquire_user_db(cx).await?;
	let data = fetch_accounts_page(conn.get())
		.await
		.map_err(|e| internal_server_error(StdbError(e)))?;
	let accounts = sendable_accounts(&data.accounts);
	let selected = pick_from(&accounts, requested);
	let can_pick = accounts.len() > 1;
	let key = new_idempotency_key();
	let selected_id = selected.map(|a| a.account_id).unwrap_or(0);
	let from_id = if selected_id == 0 {
		String::new()
	} else {
		selected_id.to_string()
	};
	let from_ledger = selected
		.map(|a| a.ledger_id.to_string())
		.unwrap_or_default();
	let signals = format!(
		"{{fromId:'{from_id}',fromLedgerId:'{from_ledger}',pickingFrom:false,recipientId:'',recipientName:'',recipientAddr:'',recipientLabel:'',recipientSearch:'',amount:'',memo:'',idempotencyKey:'{key}',sendError:'',sent:false,sentQty:'',sentTo:'',sentAddr:''}}"
	);

	view! {
		<main
			id="page-content"
			class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
			data-signals=(signals)
		>
			<h1 class="mb-6 text-2xl font-medium md:text-3xl">"Send"</h1>
			if accounts.is_empty() {
				empty_state()
			} else {
				send_form(
					accounts: accounts,
					can_pick: can_pick,
					selected_id: selected_id,
				)
			}
		</main>
	}
}

#[component]
async fn empty_state() -> Result {
	view! {
		<div class="rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center">
			<p class="text-neutral-300">"You need a writable account to send."</p>
			<p class="mt-2 text-sm text-neutral-400">
				"Create a debit account, or ask an owner to grant you Write."
			</p>
			<a
				href="/app/accounts"
				class="mt-6 inline-flex rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
			>
				"Create an account"
			</a>
		</div>
	}
}

#[component]
async fn send_form(accounts: Vec<MyAccountRow>, can_pick: bool, selected_id: u64) -> Result {
	view! {
		<div data-show="!$sent">
			from_panel(accounts: accounts.clone(), can_pick: can_pick, selected_id: selected_id)

			<div class="mt-5" data-show="$fromId != ''">
				<p class="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-400">
					"To"
				</p>
				<div data-show="!$recipientId">
					<input
						type="text"
						class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
						data-bind="recipientSearch"
						placeholder="@username or #address"
						autocomplete="off"
						autocapitalize="off"
						spellcheck="false"
						data-on:input__debounce.300ms="@get('/app/transfer/recipients')"
					>
					<p class="mt-1 text-xs text-neutral-400">
						"Same asset as the sending account. Prefix @ or # to search only that kind."
					</p>
					<div id="recipient-results"></div>
				</div>
				<div
					class="flex items-center justify-between rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2.5"
					data-show="$recipientId"
					style="display: none"
				>
					<div class="min-w-0">
						<p
							class="truncate text-sm text-neutral-100"
							data-show="$recipientLabel"
							data-text="$recipientLabel"
							style="display: none"
						></p>
						<p class="truncate text-sm text-neutral-100" data-text="$recipientName"></p>
						<p
							class="truncate text-xs text-neutral-400"
							data-show="$recipientAddr"
							data-text="$recipientAddr"
							style="display: none"
						></p>
					</div>
					<button
						type="button"
						class="ml-3 shrink-0 cursor-pointer text-sm text-neutral-300 hover:text-white"
						data-on:click="$recipientId = ''; $recipientName = ''; $recipientAddr = ''; $recipientLabel = ''; $recipientSearch = ''; $sendError = ''"
					>
						"Clear"
					</button>
				</div>
			</div>

			<div class="mt-5" data-show="$fromId != ''">
				<p class="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-400">
					"Amount"
				</p>
				<div class="flex items-center gap-2">
					<input
						type="text"
						inputmode="decimal"
						class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
						data-bind="amount"
						placeholder="0"
						autocomplete="off"
					>
					amount_suffixes(accounts: accounts.clone(), selected_id: selected_id)
				</div>
				amount_hints(accounts: accounts, selected_id: selected_id)
			</div>

			<div class="mt-5" data-show="$fromId != ''">
				<div class="mb-2 flex items-baseline justify-between">
					<p class="text-xs font-medium uppercase tracking-wide text-neutral-400">
						"Memo "
						<span class="font-normal normal-case tracking-normal text-neutral-400">
							"(optional)"
						</span>
					</p>
					<p
						class="text-xs text-neutral-400"
						data-text="$memo.length + '/32'"
					></p>
				</div>
				<input
					type="text"
					class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
					data-bind="memo"
					maxlength="32"
					placeholder="What's this for?"
					autocomplete="off"
				>
			</div>

			<input type="hidden" data-bind="idempotencyKey">

			<button
				type="button"
				class="mt-6 w-full cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
				data-on:click="@post('/app/transfer')"
				data-indicator="sending"
				data-attr-disabled="$sending || !$fromId || !$recipientId || $amount == ''"
			>
				"Send"
			</button>
			<p
				class="mt-3 text-sm text-red-400"
				data-show="$sendError"
				data-text="$sendError"
			></p>
		</div>

		<div data-show="$sent" style="display: none">
			<div class="rounded-lg border border-neutral-800 bg-neutral-950 px-4 py-6 sm:px-5">
				<h2 class="text-xl font-medium">
					"Sent "
					<span data-text="$sentQty"></span>
				</h2>
				<p class="mt-1 text-sm text-neutral-300">
					"to "
					<span data-text="$sentTo"></span>
					<span data-show="$sentAddr">
						" · "
						<span data-text="$sentAddr"></span>
					</span>
				</p>
				<button
					type="button"
					class="mt-5 w-full cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
					data-on:click="window.location.href = '/app/transfer?from=' + $fromId"
				>
					"Send another"
				</button>
			</div>
		</div>
	}
}

#[component]
async fn amount_suffixes(accounts: Vec<MyAccountRow>, selected_id: u64) -> Result {
	view! {
		for acc in &accounts {
			ledger_suffix(acc: acc, selected: acc.account_id == selected_id)
		}
	}
}

#[component]
async fn ledger_suffix(acc: &MyAccountRow, selected: bool) -> Result {
	let show = format!("$fromId == '{}'", acc.account_id);
	if selected {
		view! {
			<span class="shrink-0 text-sm text-neutral-300" data-show=(show)>
				(acc.ledger_name.clone())
			</span>
		}
	} else {
		view! {
			<span
				class="shrink-0 text-sm text-neutral-300"
				data-show=(show)
				style="display: none"
			>
				(acc.ledger_name.clone())
			</span>
		}
	}
}

#[component]
async fn amount_hints(accounts: Vec<MyAccountRow>, selected_id: u64) -> Result {
	view! {
		for acc in &accounts {
			avail_hint(acc: acc, selected: acc.account_id == selected_id)
		}
	}
}

#[component]
async fn avail_hint(acc: &MyAccountRow, selected: bool) -> Result {
	let show = format!("$fromId == '{}'", acc.account_id);
	let qty = crate::stdb::format_qty(acc.balance, acc.ledger_scale);
	if selected {
		view! {
			<p class="mt-1 text-xs text-neutral-400" data-show=(show)>
				"Available "
				<span class="text-anakiwa">(qty)</span>
			</p>
		}
	} else {
		view! {
			<p
				class="mt-1 text-xs text-neutral-400"
				data-show=(show)
				style="display: none"
			>
				"Available "
				<span class="text-anakiwa">(qty)</span>
			</p>
		}
	}
}

/// `POST /app/transfer` — command only. Success flips `$sent`.
#[route(POST)]
async fn create(cx: &Cx, Signals(form): Signals<SendSignals>) -> Result<PatchSignals> {
	let _user = require_user(cx).await?;
	let Ok(from_id) = form.from_id.trim().parse::<u64>() else {
		return send_err("Choose an account to send from.", &form, None);
	};
	let Ok(recipient_id) = form.recipient_id.trim().parse::<u64>() else {
		return send_err("Pick a recipient.", &form, None);
	};
	if form.idempotency_key.trim().is_empty() {
		return send_err("Refresh the page and try again.", &form, None);
	}

	let conn = acquire_user_db(cx).await?;
	let data = match fetch_accounts_page(conn.get()).await {
		Ok(d) => d,
		Err(e) => return send_err(&e, &form, None),
	};
	let sendable = sendable_accounts(&data.accounts);
	let Some(from) = sendable.iter().find(|a| a.account_id == from_id) else {
		return send_err("You can't send from that account.", &form, None);
	};

	let amount = match parse_qty(&form.amount, from.ledger_scale) {
		Ok(0) => return send_err("Enter an amount greater than zero.", &form, None),
		Ok(n) => n,
		Err(e) => return send_err(&e, &form, None),
	};
	if amount > from.balance {
		return send_err("Not enough available balance.", &form, None);
	}

	let memo = {
		let t = form.memo.trim();
		if t.is_empty() {
			None
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
		Ok(()) => {
			let sent_qty = format!(
				"{} {}",
				crate::stdb::format_qty(amount, from.ledger_scale),
				from.ledger_name
			);
			let sent_to = if form.recipient_name.trim().is_empty() {
				form.recipient_id.clone()
			} else {
				form.recipient_name.clone()
			};
			PatchSignals::json(&SendPatch {
				send_error: String::new(),
				sent: true,
				sent_qty,
				sent_to,
				sent_addr: form.recipient_addr.clone(),
				idempotency_key: new_idempotency_key(),
				amount: String::new(),
				memo: String::new(),
				recipient_id: String::new(),
				recipient_name: String::new(),
				recipient_addr: String::new(),
				recipient_label: String::new(),
				recipient_search: String::new(),
			})
		}
		Err(e) => send_err(&map_transfer_error(&e), &form, None),
	}
}

fn pick_from(accounts: &[MyAccountRow], requested: Option<u64>) -> Option<&MyAccountRow> {
	if let Some(id) = requested {
		if let Some(a) = accounts.iter().find(|a| a.account_id == id) {
			return Some(a);
		}
	}
	accounts.iter().find(|a| a.is_primary).or(accounts.first())
}

#[derive(Debug, Deserialize)]
struct SendSignals {
	#[serde(default, rename = "fromId")]
	from_id: String,
	#[serde(default, rename = "recipientId")]
	recipient_id: String,
	#[serde(default, rename = "recipientName")]
	recipient_name: String,
	#[serde(default, rename = "recipientAddr")]
	recipient_addr: String,
	#[serde(default, rename = "recipientLabel")]
	recipient_label: String,
	#[serde(default)]
	amount: String,
	#[serde(default)]
	memo: String,
	#[serde(default, rename = "idempotencyKey")]
	idempotency_key: String,
}

#[derive(Serialize)]
struct SendPatch {
	#[serde(rename = "sendError")]
	send_error: String,
	sent: bool,
	#[serde(rename = "sentQty")]
	sent_qty: String,
	#[serde(rename = "sentTo")]
	sent_to: String,
	#[serde(rename = "sentAddr")]
	sent_addr: String,
	#[serde(rename = "idempotencyKey")]
	idempotency_key: String,
	amount: String,
	memo: String,
	#[serde(rename = "recipientId")]
	recipient_id: String,
	#[serde(rename = "recipientName")]
	recipient_name: String,
	#[serde(rename = "recipientAddr")]
	recipient_addr: String,
	#[serde(rename = "recipientLabel")]
	recipient_label: String,
	#[serde(rename = "recipientSearch")]
	recipient_search: String,
}

fn send_err(error: &str, form: &SendSignals, key: Option<String>) -> Result<PatchSignals> {
	PatchSignals::json(&SendPatch {
		send_error: error.to_owned(),
		sent: false,
		sent_qty: String::new(),
		sent_to: String::new(),
		sent_addr: String::new(),
		idempotency_key: key.unwrap_or_else(|| form.idempotency_key.clone()),
		amount: form.amount.clone(),
		memo: form.memo.clone(),
		recipient_id: form.recipient_id.clone(),
		recipient_name: form.recipient_name.clone(),
		recipient_addr: form.recipient_addr.clone(),
		recipient_label: form.recipient_label.clone(),
		recipient_search: String::new(),
	})
}
