//! Shared deposit / withdraw form (Issue / Redeem). Routes live on those pages;
//! counterpart search is `GET /app/issue/counterparts`.

use super::transfer::markup::{from_panel, recipient_results_html};
use crate::auth::require_user;
use crate::module_bindings::{AccountKind, MyAccountRow};
use crate::stdb::{
	StdbError, acquire_user_db, counterpart_kind, create_user_transfer, fetch_accounts_page,
	flow_accounts, format_qty, lookup_account, map_transfer_error, new_idempotency_key, own_issuer,
	parse_qty, pick_flow, search_directory,
};
use serde::{Deserialize, Serialize};
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchElements, PatchSignals, Signals},
	router::{error::internal_server_error, query_params, route},
	view::{component, view},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
	Deposit,
	Withdraw,
}

impl Flow {
	fn title(self) -> &'static str {
		match self {
			Self::Deposit => "Deposit",
			Self::Withdraw => "Withdraw",
		}
	}

	fn action_path(self) -> &'static str {
		match self {
			Self::Deposit => "/app/deposit",
			Self::Withdraw => "/app/withdraw",
		}
	}

	fn success_posted(self) -> &'static str {
		match self {
			Self::Deposit => "Deposited",
			Self::Withdraw => "Withdrawn",
		}
	}

	fn empty_blurb(self) -> &'static str {
		match self {
			Self::Deposit => "Create a debit account, or use an issuer credit you can write.",
			Self::Withdraw => "Create a debit account, or use an issuer credit you can write.",
		}
	}
}

#[query_params]
struct FlowQuery {
	from: Option<String>,
}

#[component]
pub async fn flow_page(cx: &Cx, flow: Flow) -> Result {
	let _user = require_user(cx).await?;
	let requested = query_params::<FlowQuery>(cx)
		.ok()
		.and_then(|q| q.from.clone())
		.and_then(|s| s.parse::<u64>().ok());

	let conn = acquire_user_db(cx).await?;
	let data = fetch_accounts_page(conn.get())
		.await
		.map_err(|e| internal_server_error(StdbError(e)))?;
	let accounts = flow_accounts(&data.accounts);
	let selected = pick_flow(&accounts, requested);
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
	let from_kind = selected.map(|a| kind_token(a.kind)).unwrap_or_default();

	let mut recipient_id = String::new();
	let mut recipient_name = String::new();
	let mut recipient_addr = String::new();
	let mut recipient_label = String::new();
	if let Some(yours) = selected {
		if let Some(issuer) = own_issuer(&accounts, yours) {
			recipient_id = issuer.account_id.to_string();
			recipient_addr = format!("#{}", issuer.address);
			if let Some(label) = issuer
				.label
				.as_deref()
				.map(str::trim)
				.filter(|s| !s.is_empty())
			{
				recipient_label = label.to_owned();
				recipient_name = recipient_addr.clone();
			} else {
				recipient_name = recipient_addr.clone();
			}
		}
	}

	let signals = format!(
		"{{fromId:'{from_id}',fromLedgerId:'{from_ledger}',fromKind:'{from_kind}',pickingFrom:false,recipientId:'{}',recipientName:'{}',recipientAddr:'{}',recipientLabel:'{}',recipientSearch:'',amount:'',memo:'',idempotencyKey:'{key}',sendError:'',sent:false,pending:false,sentQty:'',sentTo:'',sentAddr:''}}",
		js_single(&recipient_id),
		js_single(&recipient_name),
		js_single(&recipient_addr),
		js_single(&recipient_label),
	);

	let title = flow.title();
	view! {
		<main
			id="page-content"
			class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
			data-signals=(signals)
		>
			<h1 class="mb-6 text-2xl font-medium md:text-3xl">(title)</h1>
			if accounts.is_empty() {
				empty_state(flow: flow)
			} else {
				flow_form(
					flow: flow,
					accounts: accounts,
					can_pick: can_pick,
					selected_id: selected_id,
				)
			}
		</main>
	}
}

#[component]
async fn empty_state(flow: Flow) -> Result {
	let blurb = flow.empty_blurb();
	view! {
		<div class="rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center">
			<p class="text-neutral-300">"You need a writable account."</p>
			<p class="mt-2 text-sm text-neutral-400">(blurb)</p>
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
async fn flow_form(
	flow: Flow,
	accounts: Vec<MyAccountRow>,
	can_pick: bool,
	selected_id: u64,
) -> Result {
	let post = format!("@post('{}')", flow.action_path());
	let cta = flow.title();
	let posted = flow.success_posted();
	let other_when_issuer = match flow {
		Flow::Deposit => "To wallet",
		Flow::Withdraw => "From wallet",
	};
	view! {
		<div data-show="!$sent">
			from_panel(accounts: accounts.clone(), can_pick: can_pick, selected_id: selected_id)

			<div class="mt-5" data-show="$fromId != ''">
				<p
					class="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-400"
					data-show="$fromKind == 'Debit'"
				>
					"Issuer"
				</p>
				<p
					class="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-400"
					data-show="$fromKind == 'Credit'"
					style="display: none"
				>
					(other_when_issuer)
				</p>
				<div data-show="!$recipientId">
					<input
						type="text"
						class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
						data-bind="recipientSearch"
						data-attr-placeholder="$fromKind == 'Debit' ? 'Issuer #address' : '@username or #address'"
						autocomplete="off"
						autocapitalize="off"
						spellcheck="false"
						data-on:input__debounce.300ms="@get('/app/issue/counterparts')"
					>
					<p class="mt-1 text-xs text-neutral-400" data-show="$fromKind == 'Debit'">
						"Same asset. Search the issuer address, e.g. #STELOBANK."
					</p>
					<p
						class="mt-1 text-xs text-neutral-400"
						data-show="$fromKind == 'Credit'"
						style="display: none"
					>
						"Same asset as the issuer account. Prefix @ or # to search only that kind."
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
					<p class="text-xs text-neutral-400" data-text="$memo.length + '/32'"></p>
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
				data-on:click=(post)
				data-indicator="sending"
				data-attr-disabled="$sending || !$fromId || !$recipientId || $amount == ''"
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
			<div class="rounded-lg border border-neutral-800 bg-neutral-950 px-4 py-6 sm:px-5">
				<h2 class="text-xl font-medium" data-show="!$pending">
					(posted)
					" "
					<span data-text="$sentQty"></span>
				</h2>
				<h2 class="text-xl font-medium" data-show="$pending" style="display: none">
					"Requested "
					<span data-text="$sentQty"></span>
				</h2>
				<p class="mt-1 text-sm text-neutral-300" data-show="!$pending">
					<span data-text="$sentTo"></span>
					<span data-show="$sentAddr">
						" · "
						<span data-text="$sentAddr"></span>
					</span>
				</p>
				<p class="mt-1 text-sm text-neutral-300" data-show="$pending" style="display: none">
					"Waiting on the issuer to confirm. "
					<span data-text="$sentTo"></span>
					<span data-show="$sentAddr">
						" · "
						<span data-text="$sentAddr"></span>
					</span>
				</p>
				<a
					href="/app/activity"
					class="mt-5 inline-flex w-full cursor-pointer items-center justify-center rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
				>
					"View activity"
				</a>
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
	let show = format!("$fromId == '{}' && $fromKind == 'Debit'", acc.account_id);
	let qty = format_qty(acc.balance, acc.ledger_scale);
	if selected && matches!(acc.kind, AccountKind::Debit) {
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

#[derive(Debug, Deserialize)]
pub struct FlowSignals {
	#[serde(default, rename = "fromId")]
	from_id: String,
	#[serde(default, rename = "recipientId")]
	recipient_id: String,
	#[serde(default, rename = "recipientName")]
	recipient_name: String,
	#[serde(default, rename = "recipientAddr")]
	recipient_addr: String,
	#[serde(default)]
	amount: String,
	#[serde(default)]
	memo: String,
	#[serde(default, rename = "idempotencyKey")]
	idempotency_key: String,
}

#[derive(Serialize)]
struct FlowPatch {
	#[serde(rename = "sendError")]
	send_error: String,
	sent: bool,
	pending: bool,
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

pub async fn flow_create(cx: &Cx, flow: Flow, form: FlowSignals) -> Result<PatchSignals> {
	let _user = require_user(cx).await?;
	let Ok(from_id) = form.from_id.trim().parse::<u64>() else {
		return flow_err("Choose an account.", &form, None);
	};
	let Ok(counterpart_id) = form.recipient_id.trim().parse::<u64>() else {
		return flow_err("Pick the other account.", &form, None);
	};
	if form.idempotency_key.trim().is_empty() {
		return flow_err("Refresh the page and try again.", &form, None);
	}

	let conn = acquire_user_db(cx).await?;
	let data = match fetch_accounts_page(conn.get()).await {
		Ok(d) => d,
		Err(e) => return flow_err(&e, &form, None),
	};
	let accounts = flow_accounts(&data.accounts);
	let Some(yours) = accounts.iter().find(|a| a.account_id == from_id) else {
		return flow_err("You can't use that account.", &form, None);
	};

	let counterpart = match lookup_account(conn.get(), counterpart_id).await {
		Ok(hit) => hit,
		Err(_) => return flow_err("That account wasn't found.", &form, None),
	};
	if counterpart.ledger_id != yours.ledger_id {
		return flow_err("That account is a different asset.", &form, None);
	}
	if counterpart.kind != counterpart_kind(yours.kind) {
		return flow_err(
			match yours.kind {
				AccountKind::Debit => "Pick an issuer (credit) account.",
				AccountKind::Credit => "Pick a player (debit) account.",
			},
			&form,
			None,
		);
	}

	let (sending_id, receiving_id) = match (flow, yours.kind) {
		(Flow::Deposit, AccountKind::Debit) => (counterpart_id, from_id),
		(Flow::Deposit, AccountKind::Credit) => (from_id, counterpart_id),
		(Flow::Withdraw, AccountKind::Debit) => (from_id, counterpart_id),
		(Flow::Withdraw, AccountKind::Credit) => (counterpart_id, from_id),
	};

	let amount = match parse_qty(&form.amount, yours.ledger_scale) {
		Ok(0) => return flow_err("Enter an amount greater than zero.", &form, None),
		Ok(n) => n,
		Err(e) => return flow_err(&e, &form, None),
	};

	if matches!(yours.kind, AccountKind::Debit)
		&& matches!(flow, Flow::Withdraw)
		&& amount > yours.balance
	{
		return flow_err("Not enough available balance.", &form, None);
	}

	let memo = {
		let t = form.memo.trim();
		if t.is_empty() {
			None
		} else {
			Some(t.to_owned())
		}
	};

	// Debit as "your account" = player request → pending; issuer confirms on Activity.
	// Credit as "your account" = issuer acting → posted. Write on both accounts
	// (typical local seed / bank operator) must not auto-post a debit-initiated flow.
	let pending = matches!(yours.kind, AccountKind::Debit);

	match create_user_transfer(
		conn.get(),
		sending_id,
		receiving_id,
		amount,
		memo,
		form.idempotency_key.trim().to_owned(),
		pending,
	)
	.await
	{
		Ok(()) => {
			let sent_qty = format!(
				"{} {}",
				format_qty(amount, yours.ledger_scale),
				yours.ledger_name
			);
			let sent_to = if form.recipient_name.trim().is_empty() {
				form.recipient_id.clone()
			} else {
				form.recipient_name.clone()
			};
			PatchSignals::json(&FlowPatch {
				send_error: String::new(),
				sent: true,
				pending,
				sent_qty,
				sent_to,
				sent_addr: form.recipient_addr.clone(),
				idempotency_key: new_idempotency_key(),
				amount: String::new(),
				memo: String::new(),
				recipient_id: form.recipient_id.clone(),
				recipient_name: form.recipient_name.clone(),
				recipient_addr: form.recipient_addr.clone(),
				recipient_label: String::new(),
				recipient_search: String::new(),
			})
		}
		Err(e) => flow_err(&map_transfer_error(&e), &form, None),
	}
}

fn flow_err(error: &str, form: &FlowSignals, key: Option<String>) -> Result<PatchSignals> {
	PatchSignals::json(&FlowPatch {
		send_error: error.to_owned(),
		sent: false,
		pending: false,
		sent_qty: String::new(),
		sent_to: String::new(),
		sent_addr: String::new(),
		idempotency_key: key.unwrap_or_else(|| form.idempotency_key.clone()),
		amount: form.amount.clone(),
		memo: form.memo.clone(),
		recipient_id: form.recipient_id.clone(),
		recipient_name: form.recipient_name.clone(),
		recipient_addr: form.recipient_addr.clone(),
		recipient_label: String::new(),
		recipient_search: String::new(),
	})
}

#[derive(Debug, Deserialize)]
struct SearchSignals {
	#[serde(default, rename = "fromId")]
	from_id: String,
	#[serde(default, rename = "fromLedgerId")]
	from_ledger_id: String,
	#[serde(default, rename = "fromKind")]
	from_kind: String,
	#[serde(default, rename = "recipientSearch")]
	recipient_search: String,
}

/// `GET /app/issue/counterparts` — kind-filtered directory search.
#[route(GET "/app/issue/counterparts")]
async fn counterparts(cx: &Cx, Signals(form): Signals<SearchSignals>) -> Result<PatchElements> {
	let _user = require_user(cx).await?;
	let term = form.recipient_search.trim();
	if term.is_empty() {
		return Ok(PatchElements::new(recipient_results_html(&[], false)));
	}
	let Ok(ledger_id) = form.from_ledger_id.trim().parse::<u64>() else {
		return Ok(PatchElements::new(recipient_results_html(&[], true)));
	};
	let exclude_id = form.from_id.trim().parse::<u64>().unwrap_or(0);
	let Some(yours_kind) = parse_kind(&form.from_kind) else {
		return Ok(PatchElements::new(recipient_results_html(&[], true)));
	};
	let want = counterpart_kind(yours_kind);

	let conn = acquire_user_db(cx).await?;
	let hits = search_directory(conn.get(), ledger_id, exclude_id, term)
		.await
		.map_err(|e| internal_server_error(StdbError(e)))?;
	let hits: Vec<_> = hits.into_iter().filter(|h| h.kind == want).collect();
	Ok(PatchElements::new(recipient_results_html(&hits, true)))
}

fn parse_kind(s: &str) -> Option<AccountKind> {
	match s.trim() {
		"Debit" => Some(AccountKind::Debit),
		"Credit" => Some(AccountKind::Credit),
		_ => None,
	}
}

fn kind_token(kind: AccountKind) -> &'static str {
	match kind {
		AccountKind::Debit => "Debit",
		AccountKind::Credit => "Credit",
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
