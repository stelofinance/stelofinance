//! `GET /app/transfer` — send. Recipient search: `recipients`.

pub(super) mod markup;
mod recipients;

use crate::domain::{
	map_transfer_error, new_idempotency_key, normalize_memo, parse_qty, sendable_accounts,
};
use crate::module_bindings::MyAccountRow;
use crate::stdb::{acquire_user_db, create_user_transfer, fetch_accounts_page};
use crate::ui::runtime::search_input;
use markup::{Recipient, SEARCH_CLASS, Sent, amount_field, from_panel, memo_field, recipient_chip};
use recipients::transfer_recipients;
use topcoat::{
	Result,
	context::Cx,
	metadata::Metadata,
	router::{error::internal_server_error, page, query_params},
	runtime::{procedure, signal},
	view::{View, component, view},
};

#[query_params]
struct SendQuery {
	from: Option<String>,
}

/// `GET /app/transfer` — from-account, recipient, amount, memo.
#[page(with(Metadata::new().title("Send · Stelo Finance")))]
async fn page(cx: &Cx) -> Result<impl View> {
	let requested = query_params::<SendQuery>(cx)
		.ok()
		.and_then(|q| q.from.clone())
		.and_then(|s| s.parse::<u64>().ok());

	let conn = acquire_user_db(cx).await?;
	let data = fetch_accounts_page(conn.get())
		.await
		.map_err(internal_server_error)?;
	let accounts = sendable_accounts(&data.accounts);
	let selected = pick_from(&accounts, requested);
	let can_pick = accounts.len() > 1;
	let selected_id = selected.map(|a| a.account_id).unwrap_or(0);
	let selected_ledger = selected.map(|a| a.ledger_id).unwrap_or(0);

	Ok(view! {
    <main
        id="page-content"
        class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
    >
        <h1 class="mb-6 text-2xl font-medium md:text-3xl">"Send"</h1>
        if accounts.is_empty() {
            empty_state()
        } else {
            send_form(
                accounts: accounts,
                can_pick: can_pick,
                selected_id: selected_id,
                selected_ledger: selected_ledger,
                key: new_idempotency_key()
            )
        }
    </main>
})
}

#[component]
async fn empty_state(cx: &Cx) -> Result<impl View> {
	Ok(view! {
    <div
        class="rounded-lg border border-neutral-800 bg-neutral-950 px-5 py-10 text-center"
    >
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
})
}

#[component]
async fn send_form(
	cx: &Cx,
	accounts: Vec<MyAccountRow>,
	can_pick: bool,
	selected_id: u64,
	selected_ledger: u64,
	key: String,
) -> Result<impl View> {
	let from_id = signal(cx, || selected_id);
	let from_ledger_id = signal(cx, || selected_ledger);
	let from_kind = signal(cx, || "debit".to_owned());
	let picking = signal(cx, || false);
	let recipient = Recipient::new(cx, 0, String::new(), String::new(), String::new());
	let amount = signal(cx, String::new);
	let memo = signal(cx, String::new);
	let key = signal(cx, || key.clone());
	let error = signal(cx, String::new);
	let busy = signal(cx, || false);
	let sent = signal(cx, || false);
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
            >
                "To"
            </p>
            <div :hidden=$(r_id.get() != 0u64)>
                search_input(
                    query: r_search.clone(),
                    class: SEARCH_CLASS,
                    placeholder: $("@username or #address".to_owned())
                )
                <p class="mt-1 text-xs text-neutral-400">
                    "Same asset as the sending account. Prefix @ or # to search only that kind."
                </p>
                transfer_recipients(
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
                let result = send(
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
                    key.set(done.key.clone());
                    error.set("".to_owned());
                    amount.set("".to_owned());
                    memo.set("".to_owned());
                    r_id.set(0u64);
                    r_name.set("".to_owned());
                    r_addr.set("".to_owned());
                    r_label.set("".to_owned());
                    r_search.set("".to_owned());
                    sent.set(true);
                } else {
                    error.set(result.unwrap_err());
                }
            })
        >
            "Send"
        </button>
        <p class="mt-3 text-sm text-red-400" :hidden=$(error.get().is_empty())>
            $(error.get())
        </p>
    </div>

    <div :hidden=$(!sent.get())>
        <div
            class="rounded-lg border border-neutral-800 bg-neutral-950 px-4 py-6 sm:px-5"
        >
            <h2 class="text-xl font-medium">
                "Sent "
                <span>$(sent_qty.get())</span>
            </h2>
            <p class="mt-1 text-sm text-neutral-300">
                "to "
                <span>$(sent_to.get())</span>
                <span :hidden=$(sent_addr.get().is_empty())>
                    " · "
                    <span>$(sent_addr.get())</span>
                </span>
            </p>
            <button
                type="button"
                class="mt-5 w-full cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
                @click=$(|_e| {
                    let id = from_id.get();
                    raw!(
                        "window.location.assign('/app/transfer?from=' + ${id}.toString())",
                        {
                            let _ = id;
                        },
                    )
                })
            >
                "Send another"
            </button>
        </div>
    </div>
})
}

/// Send from one of the user's debit accounts. Failures come back as
/// `Err(message)` data; the key stays the same so a retry is deduplicated,
/// and only a success returns a fresh one.
#[procedure("/app/transfer/send")]
async fn send(
	cx: &Cx,
	from_id: u64,
	recipient_id: u64,
	amount: String,
	memo: String,
	idempotency_key: String,
) -> Result<std::result::Result<Sent, String>> {
	if from_id == 0 {
		return Ok(Err("Choose an account to send from.".into()));
	}
	if recipient_id == 0 {
		return Ok(Err("Pick a recipient.".into()));
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
	let sendable = sendable_accounts(&data.accounts);
	let Some(from) = sendable.iter().find(|a| a.account_id == from_id) else {
		return Ok(Err("You can't send from that account.".into()));
	};

	let amount = match parse_qty(&amount, from.ledger_scale) {
		Ok(0) => return Ok(Err("Enter an amount greater than zero.".into())),
		Ok(n) => n,
		Err(e) => return Ok(Err(e)),
	};
	if amount > from.balance {
		return Ok(Err("Not enough available balance.".into()));
	}

	let Ok(memo) = normalize_memo(&memo) else {
		return Ok(Err("Memo is too long.".into()));
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
		Ok(()) => Ok(Ok(Sent {
			qty: format!(
				"{} {}",
				crate::domain::format_qty(amount, from.ledger_scale),
				from.ledger_name
			),
			pending: false,
			key: new_idempotency_key(),
		})),
		Err(e) => Ok(Err(map_transfer_error(e.user_message()))),
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
