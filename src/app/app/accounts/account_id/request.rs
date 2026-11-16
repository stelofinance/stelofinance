//! Build a shareable payment-request link (Read+).

use crate::domain::{MAX_MEMO_LEN, normalize_memo, parse_qty};
use crate::stdb::account::fetch_account_home;
use crate::stdb::acquire_user_db;
use crate::ui::runtime::copy_button;
use topcoat::{
	Result,
	context::Cx,
	runtime::{Event, procedure, signal},
	view::{View, component, view},
};

/// Static suffix of the memo counter; keep in sync with `MAX_MEMO_LEN`.
const MEMO_MAX: &str = "/64";

const _: () = assert!(MAX_MEMO_LEN == 64);

/// Amount + memo form, then the link once created. Owns its signals, and is
/// outside the live region so typing survives live updates.
#[component]
pub async fn request_form(
	cx: &Cx,
	account_id: u64,
	ledger_name: String,
	debit: bool,
) -> Result<impl View> {
	let amount = signal(cx, String::new);
	let memo = signal(cx, String::new);
	let link = signal(cx, String::new);
	let error = signal(cx, String::new);
	let busy = signal(cx, || false);
	let helper = if debit {
		"Share a link so someone can pay this account."
	} else {
		"Share a link so someone can pay this issuer account."
	};
	Ok(view! {
    <section class="mt-10">
        <div :hidden=$(link.get() != "")>
            <h2 class="text-sm font-medium uppercase tracking-wide text-neutral-400">
                "Request"
            </h2>
            <p class="mt-2 text-sm text-neutral-400">(helper)</p>
            <p
                class="mb-2 mt-4 text-xs font-medium uppercase tracking-wide text-neutral-400"
            >
                "Amount"
            </p>
            <div class="flex items-center gap-2">
                <input
                    type="text"
                    inputmode="decimal"
                    class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
                    :value=$(amount.get())
                    @input=$(|e: Event| amount.set(e.target.value))
                    placeholder="0"
                    autocomplete="off"
                >
                <span class="shrink-0 text-sm text-neutral-300">
                    (ledger_name.clone())
                </span>
            </div>
            <div class="mt-4">
                <div class="mb-2 flex items-baseline justify-between">
                    <p
                        class="text-xs font-medium uppercase tracking-wide text-neutral-400"
                    >
                        "Memo "
                        <span
                            class="font-normal normal-case tracking-normal text-neutral-400"
                        >
                            "(optional)"
                        </span>
                    </p>
                    <p class="text-xs text-neutral-400">
                        $(memo.get().len())
                        (MEMO_MAX)
                    </p>
                </div>
                <input
                    type="text"
                    class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
                    :value=$(memo.get())
                    @input=$(|e: Event| memo.set(e.target.value))
                    maxlength=(MAX_MEMO_LEN)
                    placeholder="What's this for?"
                    autocomplete="off"
                >
            </div>
            <button
                type="button"
                class="mt-4 cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
                :disabled=$(if busy.get() { true } else { amount.get() == "" })
                @click=$(async |_e| {
                    busy.set(true);
                    let result = create_request_link(
                        account_id,
                        amount.get(),
                        memo.get(),
                    ).await;
                    busy.set(false);
                    if result.is_ok() {
                        link.set(result.unwrap());
                        error.set("".to_owned());
                    } else {
                        link.set("".to_owned());
                        error.set(result.unwrap_err());
                    }
                })
            >
                "Create link"
            </button>
            <p class="mt-3 text-sm text-red-400" :hidden=$(error.get().is_empty())>
                $(error.get())
            </p>
        </div>
        <div
            class="rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
            :hidden=$(link.get() == "")
        >
            <h2 class="text-lg font-medium">"Payment link"</h2>
            <p class="mt-1 text-sm text-neutral-300">
                "Send this to whoever should pay."
            </p>
            <div
                class="mt-4 flex items-stretch overflow-hidden rounded-lg border border-neutral-800"
            >
                <code
                    class="min-w-0 flex-1 truncate bg-neutral-900 px-3 py-2 text-sm text-anakiwa"
                >
                    $({
                        let l = link.get();
                        raw!("window.location.origin + ${l}", l.to_owned())
                    })
                </code>
                copy_button(
                    text: $(link.get()),
                    origin: true,
                    class: "shrink-0 cursor-pointer border-l border-neutral-800 px-3 text-xs text-neutral-300 hover:bg-neutral-900 hover:text-white"
                )
            </div>
            <div class="mt-4 flex flex-wrap gap-2">
                <a
                    class="cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
                    :href=$(link.get())
                >
                    "Preview"
                </a>
                <button
                    type="button"
                    class="cursor-pointer rounded-md bg-neutral-800 px-4 py-2 text-sm hover:bg-neutral-700"
                    @click=$(|_e| {
                        link.set("".to_owned());
                        amount.set("".to_owned());
                        memo.set("".to_owned());
                        error.set("".to_owned());
                    })
                >
                    "New"
                </button>
            </div>
        </div>
    </section>
})
}

/// Validate the form and return the link's path + query. Any Read+ member
/// may build one; the account lookup only returns accounts they belong to.
#[procedure("/app/accounts/request-link")]
async fn create_request_link(
	cx: &Cx,
	account_id: u64,
	amount: String,
	memo: String,
) -> Result<std::result::Result<String, String>> {
	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err("Your session expired. Reload the page.".into()));
	};
	let data = match fetch_account_home(conn.get(), account_id).await {
		Ok(d) => d,
		Err(e) => return Ok(Err(e.user_message().to_owned())),
	};
	let Some(acc) = data.account else {
		return Ok(Err("Account not found.".into()));
	};

	let amount = match parse_qty(&amount, acc.ledger_scale) {
		Ok(0) => return Ok(Err("Enter an amount greater than zero.".into())),
		Ok(n) => n,
		Err(e) => return Ok(Err(e)),
	};

	let Ok(memo) = normalize_memo(&memo) else {
		return Ok(Err("Memo is too long.".into()));
	};

	Ok(Ok(request_link_path(
		acc.ledger_id,
		acc.account_id,
		amount,
		memo.as_deref(),
	)))
}

/// Path + query for `/app/request`. Amount is ledger base units. Memo is omitted when empty.
pub fn request_link_path(
	ledger_id: u64,
	recipient_id: u64,
	amount: u64,
	memo: Option<&str>,
) -> String {
	let mut path =
		format!("/app/request?ledgerid={ledger_id}&recipientid={recipient_id}&amount={amount}");
	if let Some(memo) = memo.map(str::trim).filter(|s| !s.is_empty()) {
		path.push_str("&memo=");
		path.push_str(&encode_query_component(memo));
	}
	path
}

/// RFC 3986 unreserved stay literal; everything else is `%HH`.
fn encode_query_component(s: &str) -> String {
	let mut out = String::new();
	for b in s.as_bytes() {
		match *b {
			b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
				out.push(*b as char);
			}
			_ => out.push_str(&format!("%{b:02X}")),
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::request_link_path;

	#[test]
	fn omits_empty_memo() {
		assert_eq!(
			request_link_path(1, 42, 5000, None),
			"/app/request?ledgerid=1&recipientid=42&amount=5000"
		);
		assert_eq!(
			request_link_path(1, 42, 5000, Some("  ")),
			"/app/request?ledgerid=1&recipientid=42&amount=5000"
		);
	}

	#[test]
	fn encodes_memo() {
		assert_eq!(
			request_link_path(1, 42, 5000, Some("Invoice #123")),
			"/app/request?ledgerid=1&recipientid=42&amount=5000&memo=Invoice%20%23123"
		);
		assert_eq!(
			request_link_path(2, 9, 1, Some("a&b=c")),
			"/app/request?ledgerid=2&recipientid=9&amount=1&memo=a%26b%3Dc"
		);
	}
}
