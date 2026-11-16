//! Shared deposit / withdraw form (Issue / Redeem). Pages live in `deposit.rs`
//! and `withdraw.rs`; the procedure is `/app/issue/send` and counterpart
//! search is the `/app/issue/counterparts` shard.

use super::transfer::markup::{
	Recipient, SEARCH_CLASS, Sent, amount_field, from_panel, memo_field, recipient_chip,
	recipient_hits,
};
use crate::domain::{
	counterpart_kind, flow_accounts, format_qty, kind_token, map_transfer_error,
	new_idempotency_key, normalize_memo, own_issuer, parse_qty, pick_flow,
};
use crate::module_bindings::{AccountKind, MyAccountRow};
use crate::stdb::{
	acquire_user_db, create_user_transfer, fetch_accounts_page, lookup_account, search_directory,
};
use crate::ui::runtime::search_input;
use topcoat::{
	Result,
	context::Cx,
	router::{error::internal_server_error, query_params},
	runtime::{Signal, procedure, shard, signal},
	view::{View, component, view},
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

	fn success_posted(self) -> &'static str {
		match self {
			Self::Deposit => "Deposited",
			Self::Withdraw => "Withdrawn",
		}
	}

	fn handling_note(self) -> &'static str {
		match self {
			Self::Deposit => "Any and all deposits are handled in person in Notsolis.",
			Self::Withdraw => "Any and all withdrawals are handled in person in Notsolis.",
		}
	}
}

const EMPTY_BLURB: &str = "Create a debit account, or use an issuer credit you can write.";

#[query_params]
struct FlowQuery {
	from: Option<String>,
}

#[component]
pub async fn flow_page(cx: &Cx, flow: Flow) -> Result<impl View> {
	let requested = query_params::<FlowQuery>(cx)
		.ok()
		.and_then(|q| q.from.clone())
		.and_then(|s| s.parse::<u64>().ok());

	let conn = acquire_user_db(cx).await?;
	let data = fetch_accounts_page(conn.get())
		.await
		.map_err(internal_server_error)?;
	let accounts = flow_accounts(&data.accounts);
	let selected = pick_flow(&accounts, requested);
	let can_pick = accounts.len() > 1;
	let selected_id = selected.map(|a| a.account_id).unwrap_or(0);
	let selected_ledger = selected.map(|a| a.ledger_id).unwrap_or(0);
	let selected_kind = selected
		.map(|a| kind_token(a.kind))
		.unwrap_or_default()
		.to_owned();

	// A debit sender starts with its own issuer as the counterpart.
	let mut recipient_id = 0;
	let mut recipient_name = String::new();
	let mut recipient_addr = String::new();
	let mut recipient_label = String::new();
	if let Some(yours) = selected {
		if let Some(issuer) = own_issuer(&accounts, yours) {
			recipient_id = issuer.account_id;
			recipient_addr = format!("#{}", issuer.address);
			recipient_name = recipient_addr.clone();
			if let Some(label) = issuer
				.label
				.as_deref()
				.map(str::trim)
				.filter(|s| !s.is_empty())
			{
				recipient_label = label.to_owned();
			}
		}
	}

	let title = flow.title();
	let handling_note = flow.handling_note();
	Ok(view! {
    <main
        id="page-content"
        class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
    >
        <h1 class="mb-3 text-2xl font-medium md:text-3xl">(title)</h1>
        <section
            class="mb-6 rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-3 sm:px-4"
        >
            <p class="text-sm text-neutral-300">(handling_note)</p>
        </section>
        if accounts.is_empty() {
            empty_state()
        } else {
            flow_form(
                flow: flow,
                accounts: accounts,
                can_pick: can_pick,
                selected_id: selected_id,
                selected_ledger: selected_ledger,
                selected_kind: selected_kind,
                recipient_id: recipient_id,
                recipient_name: recipient_name,
                recipient_addr: recipient_addr,
                recipient_label: recipient_label,
                key: new_idempotency_key()
            )
        }
    </main>
})
}

#[component]
async fn empty_state(cx: &Cx) -> Result<impl View> {
	let blurb = EMPTY_BLURB;
	Ok(view! {
    <div
        class="rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center"
    >
        <p class="text-neutral-300">"You need a writable account."</p>
        <p class="mt-2 text-sm text-neutral-400">(blurb)</p>
        <a
            href="/app/accounts"
            class="mt-6 inline-flex rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
        >
            "Accounts"
        </a>
    </div>
})
}

#[component]
async fn flow_form(
	cx: &Cx,
	flow: Flow,
	accounts: Vec<MyAccountRow>,
	can_pick: bool,
	selected_id: u64,
	selected_ledger: u64,
	selected_kind: String,
	recipient_id: u64,
	recipient_name: String,
	recipient_addr: String,
	recipient_label: String,
	key: String,
) -> Result<impl View> {
	let cta = flow.title();
	let posted = flow.success_posted();
	let deposit = matches!(flow, Flow::Deposit);
	let other_when_issuer = match flow {
		Flow::Deposit => "To wallet",
		Flow::Withdraw => "From wallet",
	};

	let from_id = signal(cx, || selected_id);
	let from_ledger_id = signal(cx, || selected_ledger);
	let from_kind = signal(cx, || selected_kind.clone());
	let picking = signal(cx, || false);
	let recipient = Recipient::new(
		cx,
		recipient_id,
		recipient_name.clone(),
		recipient_addr.clone(),
		recipient_label.clone(),
	);
	let amount = signal(cx, String::new);
	let memo = signal(cx, String::new);
	let key = signal(cx, || key.clone());
	let error = signal(cx, String::new);
	let busy = signal(cx, || false);
	let sent = signal(cx, || false);
	let pending = signal(cx, || false);
	let sent_qty = signal(cx, String::new);
	let sent_to = signal(cx, String::new);
	let sent_addr = signal(cx, String::new);
	let r_id = recipient.id.clone();
	let r_name = recipient.name.clone();
	let r_addr = recipient.addr.clone();
	let r_label = recipient.label.clone();
	let r_search = recipient.search.clone();

	Ok(view! {
    <div :hidden=$(sent.get())>
        from_panel(
            accounts: accounts.clone(),
            can_pick: can_pick,
            from_id: from_id.clone(),
            from_ledger_id: from_ledger_id.clone(),
            from_kind: from_kind.clone(),
            picking: picking.clone(),
            recipient: recipient.clone(),
            error: error.clone()
        )

        <div class="mt-5" :hidden=$(from_id.get() == 0u64)>
            <p
                class="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-400"
                :hidden=$(from_kind.get() != "debit")
            >
                "Issuer"
            </p>
            <p
                class="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-400"
                :hidden=$(from_kind.get() != "credit")
            >
                (other_when_issuer)
            </p>
            <div :hidden=$(r_id.get() != 0u64)>
                search_input(
                    query: r_search.clone(),
                    class: SEARCH_CLASS,
                    placeholder: $(if from_kind.get() == "debit" {
                        "Issuer #address".to_owned()
                    } else {
                        "@username or #address".to_owned()
                    })
                )
                <p
                    class="mt-1 text-xs text-neutral-400"
                    :hidden=$(from_kind.get() != "debit")
                >
                    "Same asset. Search the issuer address, e.g. #STELOBANK."
                </p>
                <p
                    class="mt-1 text-xs text-neutral-400"
                    :hidden=$(from_kind.get() != "credit")
                >
                    "Same asset as the issuer account. Prefix @ or # to search only that kind."
                </p>
                counterparts(
                    query: $(r_search.get()),
                    from_id: $(from_id.get()),
                    recipient_id: r_id.clone(),
                    recipient_name: r_name.clone(),
                    recipient_addr: r_addr.clone(),
                    recipient_label: r_label.clone(),
                    recipient_search: r_search.clone(),
                    error: error.clone()
                )
            </div>
            recipient_chip(recipient: recipient.clone(), error: error.clone())
        </div>

        <div class="mt-5" :hidden=$(from_id.get() == 0u64)>
            amount_field(
                accounts: accounts,
                from_id: from_id.clone(),
                amount: amount.clone()
            )
        </div>

        <div class="mt-5" :hidden=$(from_id.get() == 0u64)>
            memo_field(memo: memo.clone())
        </div>

        <button
            type="button"
            class="mt-6 w-full cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled=$(if busy.get() {
                true
            } else if from_id.get() == 0u64 {
                true
            } else if r_id.get() == 0u64 {
                true
            } else {
                amount.get() == ""
            })
            @click=$(async |_e| {
                busy.set(true);
                // Captured before the await: a second click's response can
                // land after the first one cleared the recipient.
                let to = r_name.get();
                let addr = r_addr.get();
                let result = issue_send(
                    deposit,
                    from_id.get(),
                    r_id.get(),
                    amount.get(),
                    memo.get(),
                    key.get(),
                ).await;
                busy.set(false);
                if result.is_ok() {
                    let done = result.unwrap();
                    sent_qty.set(done.qty.clone());
                    sent_to.set(to);
                    sent_addr.set(addr);
                    pending.set(done.pending);
                    key.set(done.key.clone());
                    error.set("".to_owned());
                    amount.set("".to_owned());
                    memo.set("".to_owned());
                    r_search.set("".to_owned());
                    sent.set(true);
                } else {
                    error.set(result.unwrap_err());
                }
            })
        >
            (cta)
        </button>
        <p class="mt-3 text-sm text-red-400" :hidden=$(error.get().is_empty())>
            $(error.get())
        </p>
    </div>

    <div :hidden=$(!sent.get())>
        <div
            class="rounded-lg border border-neutral-800 bg-neutral-950 px-4 py-6 sm:px-5"
        >
            <h2 class="text-xl font-medium" :hidden=$(pending.get())>
                (posted)
                " "
                <span>$(sent_qty.get())</span>
            </h2>
            <h2 class="text-xl font-medium" :hidden=$(!pending.get())>
                "Requested "
                <span>$(sent_qty.get())</span>
            </h2>
            <p class="mt-1 text-sm text-neutral-300" :hidden=$(pending.get())>
                <span>$(sent_to.get())</span>
                <span :hidden=$(sent_addr.get().is_empty())>
                    " · "
                    <span>$(sent_addr.get())</span>
                </span>
            </p>
            <p class="mt-1 text-sm text-neutral-300" :hidden=$(!pending.get())>
                "Waiting on the issuer to confirm. "
                <span>$(sent_to.get())</span>
                <span :hidden=$(sent_addr.get().is_empty())>
                    " · "
                    <span>$(sent_addr.get())</span>
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
})
}

/// Deposit or withdraw between one of the user's accounts and a counterpart
/// on the same asset. Failures come back as `Err(message)` data; the key stays
/// the same on failure so a retry is deduplicated.
#[procedure("/app/issue/send")]
async fn issue_send(
	cx: &Cx,
	deposit: bool,
	from_id: u64,
	recipient_id: u64,
	amount: String,
	memo: String,
	idempotency_key: String,
) -> Result<std::result::Result<Sent, String>> {
	let flow = if deposit {
		Flow::Deposit
	} else {
		Flow::Withdraw
	};
	let counterpart_id = recipient_id;
	if from_id == 0 {
		return Ok(Err("Choose an account.".into()));
	}
	if counterpart_id == 0 {
		return Ok(Err("Pick the other account.".into()));
	}
	if idempotency_key.trim().is_empty() {
		return Ok(Err("Refresh the page and try again.".into()));
	}

	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err("Your session expired. Reload the page.".into()));
	};
	let data = match fetch_accounts_page(conn.get()).await {
		Ok(d) => d,
		Err(e) => return Ok(Err(e.user_message().to_owned())),
	};
	let accounts = flow_accounts(&data.accounts);
	let Some(yours) = accounts.iter().find(|a| a.account_id == from_id) else {
		return Ok(Err("You can't use that account.".into()));
	};

	let counterpart = match lookup_account(conn.get(), counterpart_id).await {
		Ok(hit) => hit,
		Err(_) => return Ok(Err("That account wasn't found.".into())),
	};
	if counterpart.ledger_id != yours.ledger_id {
		return Ok(Err("That account is a different asset.".into()));
	}
	if counterpart.kind != counterpart_kind(yours.kind) {
		return Ok(Err(match yours.kind {
			AccountKind::Debit => "Pick an issuer (credit) account.",
			AccountKind::Credit => "Pick a player (debit) account.",
		}
		.into()));
	}

	let (sending_id, receiving_id) = match (flow, yours.kind) {
		(Flow::Deposit, AccountKind::Debit) => (counterpart_id, from_id),
		(Flow::Deposit, AccountKind::Credit) => (from_id, counterpart_id),
		(Flow::Withdraw, AccountKind::Debit) => (from_id, counterpart_id),
		(Flow::Withdraw, AccountKind::Credit) => (counterpart_id, from_id),
	};

	let amount = match parse_qty(&amount, yours.ledger_scale) {
		Ok(0) => return Ok(Err("Enter an amount greater than zero.".into())),
		Ok(n) => n,
		Err(e) => return Ok(Err(e)),
	};

	if matches!(yours.kind, AccountKind::Debit)
		&& matches!(flow, Flow::Withdraw)
		&& amount > yours.balance
	{
		return Ok(Err("Not enough available balance.".into()));
	}

	let Ok(memo) = normalize_memo(&memo) else {
		return Ok(Err("Memo is too long.".into()));
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
		idempotency_key.trim().to_owned(),
		pending,
	)
	.await
	{
		Ok(()) => Ok(Ok(Sent {
			qty: format!(
				"{} {}",
				format_qty(amount, yours.ledger_scale),
				yours.ledger_name
			),
			pending,
			key: new_idempotency_key(),
		})),
		Err(e) => Ok(Err(map_transfer_error(e.user_message()))),
	}
}

/// Counterpart search, kind-filtered to the opposite of the user's account
/// `from_id`. The ledger and kind come from that account, never from the
/// client; an account the user cannot use yields no hits.
#[shard("/app/issue/counterparts")]
async fn counterparts(
	cx: &Cx,
	query: String,
	from_id: u64,
	recipient_id: Signal<u64>,
	recipient_name: Signal<String>,
	recipient_addr: Signal<String>,
	recipient_label: Signal<String>,
	recipient_search: Signal<String>,
	error: Signal<String>,
) -> Result<impl View> {
	let term = query.trim();
	let hits = if term.is_empty() {
		None
	} else {
		let conn = acquire_user_db(cx).await?;
		let data = fetch_accounts_page(conn.get())
			.await
			.map_err(internal_server_error)?;
		let accounts = flow_accounts(&data.accounts);
		Some(match accounts.iter().find(|a| a.account_id == from_id) {
			Some(yours) => {
				let want = counterpart_kind(yours.kind);
				search_directory(conn.get(), yours.ledger_id, from_id, term)
					.await
					.map_err(internal_server_error)?
					.into_iter()
					.filter(|h| h.kind == want)
					.collect()
			}
			None => Vec::new(),
		})
	};
	let recipient = Recipient {
		id: recipient_id,
		name: recipient_name,
		addr: recipient_addr,
		label: recipient_label,
		search: recipient_search,
	};
	Ok(view! { recipient_hits(hits: hits, recipient: recipient, error: error) })
}
