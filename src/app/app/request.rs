//! `GET/POST /app/request` — pay a payment-request link.

use super::transfer::markup::{Recipient, from_panel};
use crate::auth::require_user;
use crate::domain::{
	format_qty, map_transfer_error, new_idempotency_key, normalize_memo, pay_from_accounts,
	pick_sendable,
};
use crate::module_bindings::{AccountSearchHit, MyAccountRow};
use crate::stdb::{acquire_user_db, create_user_transfer, fetch_accounts_page, lookup_account};
use topcoat::{
	Result,
	context::Cx,
	metadata::Metadata,
	router::{error::internal_server_error, page, query_params},
	runtime::{Signal, procedure, signal},
	view::{View, ViewExt, component, view},
};

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

/// `GET /app/request` — confirm a query-prefilled payment.
#[page(with(Metadata::new().title("Pay · Stelo Finance")))]
async fn page(cx: &Cx) -> Result<impl View> {
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
		Some(s) => s,
		None => {
			return Ok(view! { invalid_link() }.boxed());
		}
	};

	let conn = acquire_user_db(cx).await?;
	let data = fetch_accounts_page(conn.get())
		.await
		.map_err(internal_server_error)?;
	let Some(ledger) = data
		.ledgers
		.iter()
		.find(|l| l.id == spec.ledger_id)
		.cloned()
	else {
		return Ok(view! { invalid_link() }.boxed());
	};

	let recipient = match lookup_account(conn.get(), spec.recipient_id).await {
		Ok(hit) if hit.ledger_id == spec.ledger_id => hit,
		Ok(_) | Err(_) => {
			return Ok(view! { invalid_link() }.boxed());
		}
	};

	let accounts = pay_from_accounts(&data.accounts, spec.ledger_id, spec.recipient_id);
	let can_pick = accounts.len() > 1;
	let selected = pick_sendable(&accounts, None);
	let selected_id = selected.map(|a| a.account_id).unwrap_or(0);
	let selected_ledger = selected.map(|a| a.ledger_id).unwrap_or(0);
	let key = new_idempotency_key();

	let invoice = Invoice {
		qty: format_qty(spec.amount, ledger.scale),
		ledger_name: ledger.name.clone(),
		recipient_name: recipient_name(&recipient),
		recipient_addr: recipient_addr(&recipient),
		memo: spec.memo.clone(),
	};

	Ok(view! {
    <main
        id="page-content"
        class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
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
                selected_ledger: selected_ledger,
                invoice: invoice,
                recipient_id: spec.recipient_id,
                amount: spec.amount,
                memo: spec.memo.clone().unwrap_or_default(),
                key: key
            )
        }
    </main>
}
	.boxed())
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
async fn invalid_link(cx: &Cx) -> Result<impl View> {
	Ok(view! {
    <main
        class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
    >
        <h1 class="mb-6 text-2xl font-medium md:text-3xl">"Pay"</h1>
        <div
            class="rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center"
        >
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
})
}

#[component]
async fn no_account(cx: &Cx, #[into] ledger_name: String) -> Result<impl View> {
	Ok(view! {
    <div
        class="mt-6 rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center"
    >
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
})
}

#[component]
async fn invoice_card(cx: &Cx, invoice: Invoice) -> Result<impl View> {
	let has_addr = !invoice.recipient_addr.is_empty();
	Ok(view! {
    <article
        class="rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-3 sm:px-4 sm:py-4"
    >
        <div class="flex items-baseline justify-between gap-3">
            <p class="text-2xl font-medium tabular-nums sm:text-3xl">
                (invoice.qty.clone())
            </p>
            <p class="shrink-0 text-sm text-neutral-300">
                (invoice.ledger_name.clone())
            </p>
        </div>
        <p class="mt-2 truncate text-sm text-neutral-300">
            "to "
            (invoice.recipient_name.clone())
            if has_addr {
                " · "
                (invoice.recipient_addr.clone())
            }
        </p>
        if let Some(memo) = &invoice.memo {
            <p class="mt-1 truncate text-sm text-neutral-400">(memo.clone())</p>
        }
    </article>
})
}

#[component]
async fn pay_form(
	cx: &Cx,
	accounts: Vec<MyAccountRow>,
	can_pick: bool,
	selected_id: u64,
	selected_ledger: u64,
	invoice: Invoice,
	recipient_id: u64,
	amount: u64,
	memo: String,
	key: String,
) -> Result<impl View> {
	let from_id = signal(cx, || selected_id);
	let from_ledger_id = signal(cx, || selected_ledger);
	let from_kind = signal(cx, || "debit".to_owned());
	let picking = signal(cx, || false);
	// The recipient is fixed by the link; the panel's "clear on ledger change"
	// resets these unused signals, never the link's recipient.
	let recipient = Recipient::new(cx, 0, String::new(), String::new(), String::new());
	let error = signal(cx, String::new);
	let busy = signal(cx, || false);
	let sent = signal(cx, || false);
	let key = signal(cx, || key.clone());
	let cta = format!("Pay {} {}", invoice.qty, invoice.ledger_name);
	let sent_to = invoice.recipient_name.clone();
	let sent_addr = invoice.recipient_addr.clone();
	let sent_qty = format!("{} {}", invoice.qty, invoice.ledger_name);

	Ok(view! {
    <div class="mt-6" :hidden=$(sent.get())>
        from_panel(
            accounts: accounts.clone(),
            can_pick: can_pick,
            from_id: from_id.clone(),
            from_ledger_id: from_ledger_id.clone(),
            from_kind: from_kind.clone(),
            picking: picking.clone(),
            recipient: recipient,
            error: error.clone()
        )
        #[key(acc.account_id)]
        for acc in &accounts {
            account_pay(
                acc: acc,
                amount: amount,
                cta: cta.clone(),
                from_id: from_id.clone(),
                busy: busy.clone(),
                error: error.clone(),
                sent: sent.clone(),
                key: key.clone(),
                recipient_id: recipient_id,
                memo: memo.clone()
            )
        }
    </div>
    <div :hidden=$(!sent.get())>
        <div
            class="mt-6 rounded-lg border border-neutral-800 bg-neutral-950 px-4 py-6 sm:px-5"
        >
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
})
}

/// The shortfall hint and Pay button for one account, visible while that
/// account is the selected one. The Pay button is disabled when the account
/// holds less than the amount, which is known per account on the server.
#[component]
async fn account_pay(
	cx: &Cx,
	acc: &MyAccountRow,
	amount: u64,
	cta: String,
	from_id: Signal<u64>,
	busy: Signal<bool>,
	error: Signal<String>,
	sent: Signal<bool>,
	key: Signal<String>,
	recipient_id: u64,
	memo: String,
) -> Result<impl View> {
	let id = acc.account_id;
	let short = acc.balance < amount;
	Ok(view! {
    <div :hidden=$(from_id.get() != id)>
        if short {
            <p class="mt-2 text-sm text-red-400">
                "Not enough available on this account."
            </p>
        }
        <button
            type="button"
            class="mt-6 w-full cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled=$(if busy.get() { true } else { short })
            @click=$(async |_e| {
                busy.set(true);
                let result = pay(id, recipient_id, amount, memo.clone(), key.get()).await;
                busy.set(false);
                if result.is_ok() {
                    error.set("".to_owned());
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
})
}

/// Pay a request link's recipient. Failures come back as `Err(message)`
/// data. The amount and memo come from the link; the payer may tamper with
/// them, and every check below still applies.
#[procedure("/app/request/pay")]
async fn pay(
	cx: &Cx,
	from_id: u64,
	recipient_id: u64,
	amount: u64,
	memo: String,
	idempotency_key: String,
) -> Result<std::result::Result<bool, String>> {
	const INVALID: &str = "This payment link isn't valid.";
	if from_id == 0 {
		return Ok(Err("Choose an account to pay from.".into()));
	}
	if recipient_id == 0 || amount == 0 {
		return Ok(Err(INVALID.into()));
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
	let recipient = match lookup_account(conn.get(), recipient_id).await {
		Ok(hit) => hit,
		Err(_) => return Ok(Err(INVALID.into())),
	};
	let from_accounts = pay_from_accounts(&data.accounts, recipient.ledger_id, recipient_id);
	let Some(from) = from_accounts.iter().find(|a| a.account_id == from_id) else {
		return Ok(Err("You can't pay from that account.".into()));
	};
	if from.ledger_id != recipient.ledger_id {
		return Ok(Err(INVALID.into()));
	}
	if amount > from.balance {
		return Ok(Err("Not enough available balance.".into()));
	}

	let Ok(memo) = normalize_memo(&memo) else {
		return Ok(Err(INVALID.into()));
	};

	match create_user_transfer(
		conn.get(),
		from_id,
		recipient_id,
		amount,
		memo,
		idempotency_key.trim().to_owned(),
		false,
	)
	.await
	{
		Ok(()) => Ok(Ok(true)),
		Err(e) => Ok(Err(map_transfer_error(e.user_message()))),
	}
}

/// Parse a payment link's query. `None` when anything is missing or invalid.
fn parse_pay_query(
	ledgerid: Option<&str>,
	recipientid: Option<&str>,
	amount: Option<&str>,
	memo: Option<&str>,
) -> Option<PaySpec> {
	let num = |s: Option<&str>| s?.trim().parse::<u64>().ok();
	let ledger_id = num(ledgerid)?;
	let recipient_id = num(recipientid)?;
	let amount = num(amount).filter(|&a| a > 0)?;
	let memo = match memo {
		None => None,
		Some(raw) => normalize_memo(raw).ok()?,
	};
	Some(PaySpec {
		ledger_id,
		recipient_id,
		amount,
		memo,
	})
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

#[cfg(test)]
mod tests {
	use super::parse_pay_query;

	#[test]
	fn missing_is_invalid() {
		assert!(parse_pay_query(None, Some("1"), Some("5"), None).is_none());
		assert!(parse_pay_query(Some(" "), Some("1"), Some("5"), None).is_none());
	}

	#[test]
	fn zero_amount_invalid() {
		assert!(parse_pay_query(Some("1"), Some("2"), Some("0"), None).is_none());
	}

	#[test]
	fn long_memo_invalid() {
		let memo = "x".repeat(crate::domain::MAX_MEMO_LEN + 1);
		assert!(parse_pay_query(Some("1"), Some("2"), Some("10"), Some(&memo)).is_none());
	}

	#[test]
	fn memo_at_limit_valid() {
		let memo = "x".repeat(crate::domain::MAX_MEMO_LEN);
		let spec = parse_pay_query(Some("1"), Some("2"), Some("10"), Some(&memo)).unwrap();
		assert_eq!(spec.memo.as_deref(), Some(memo.as_str()));
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
