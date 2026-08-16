//! Build a shareable H4 payment-request link (Read+).

use super::AccountId;
use crate::auth::require_user;
use crate::stdb::account::fetch_account_home;
use crate::stdb::{acquire_user_db, parse_qty};
use serde::{Deserialize, Serialize};
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchSignals, Signals},
	router::{path_param, route},
	view::{component, view},
};

const MAX_MEMO_LEN: usize = 32;

/// Amount + memo form. Lives outside remorph targets so typing survives `/updates`.
#[component]
pub async fn request_form(account_id: u64, ledger_name: String, debit: bool) -> Result {
	let create = format!("@post('/app/accounts/{account_id}/request')");
	let helper = if debit {
		"Share a link so someone can pay this account."
	} else {
		"Share a link so someone can pay this issuer account."
	};
	view! {
		<section class="mt-10">
			<div data-show="!$requestLink">
				<h2 class="text-sm font-medium uppercase tracking-wide text-neutral-400">
					"Request"
				</h2>
				<p class="mt-2 text-sm text-neutral-400">
					(helper)
				</p>
				<p class="mb-2 mt-4 text-xs font-medium uppercase tracking-wide text-neutral-400">
					"Amount"
				</p>
				<div class="flex items-center gap-2">
					<input
						type="text"
						inputmode="decimal"
						class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
						data-bind="requestAmount"
						placeholder="0"
						autocomplete="off"
					>
					<span class="shrink-0 text-sm text-neutral-300">
						(ledger_name.clone())
					</span>
				</div>
				<div class="mt-4">
					<div class="mb-2 flex items-baseline justify-between">
						<p class="text-xs font-medium uppercase tracking-wide text-neutral-400">
							"Memo "
							<span class="font-normal normal-case tracking-normal text-neutral-400">
								"(optional)"
							</span>
						</p>
						<p
							class="text-xs text-neutral-400"
							data-text="$requestMemo.length + '/32'"
						></p>
					</div>
					<input
						type="text"
						class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm text-white placeholder:text-neutral-400"
						data-bind="requestMemo"
						maxlength="32"
						placeholder="What's this for?"
						autocomplete="off"
					>
				</div>
				<button
					type="button"
					class="mt-4 cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
					data-on:click=(create)
					data-indicator="creatingRequest"
					data-attr-disabled="$creatingRequest || $requestAmount == ''"
				>
					"Create link"
				</button>
				<p
					class="mt-3 text-sm text-red-400"
					data-show="$requestError"
					data-text="$requestError"
				></p>
			</div>
			<div
				class="rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
				data-show="$requestLink"
				style="display: none"
			>
				<h2 class="text-lg font-medium">
					"Payment link"
				</h2>
				<p class="mt-1 text-sm text-neutral-300">
					"Send this to whoever should pay."
				</p>
				<div class="mt-4 flex items-stretch overflow-hidden rounded-lg border border-neutral-800">
					<code
						class="min-w-0 flex-1 truncate bg-neutral-900 px-3 py-2 text-sm text-anakiwa"
						data-text="window.location.origin + $requestLink"
					></code>
					<button
						type="button"
						class="shrink-0 cursor-pointer border-l border-neutral-800 px-3 text-xs text-neutral-300 hover:bg-neutral-900 hover:text-white"
						data-on:click="window.navigator.clipboard.writeText(window.location.origin + $requestLink); $requestCopied = true"
						data-on:click__delay.2000ms="$requestCopied = false"
						data-text="$requestCopied ? 'Copied' : 'Copy'"
					>
						"Copy"
					</button>
				</div>
				<div class="mt-4 flex flex-wrap gap-2">
					<a
						class="cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
						data-attr-href="$requestLink"
					>
						"Preview"
					</a>
					<button
						type="button"
						class="cursor-pointer rounded-md bg-neutral-800 px-4 py-2 text-sm hover:bg-neutral-700"
						data-on:click="$requestLink = ''; $requestAmount = ''; $requestMemo = ''; $requestError = ''; $requestCopied = false"
					>
						"New"
					</button>
				</div>
			</div>
		</section>
	}
}

/// `POST /app/accounts/{account_id}/request` — path+query in `$requestLink`.
#[route(POST)]
async fn create_request_link(
	cx: &Cx,
	Signals(form): Signals<RequestSignals>,
) -> Result<PatchSignals> {
	let _user = require_user(cx).await?;
	let account_id = *path_param::<AccountId>(cx)?;
	let conn = acquire_user_db(cx).await?;
	let data = match fetch_account_home(conn.get(), account_id).await {
		Ok(d) => d,
		Err(e) => return request_err(&e),
	};
	let Some(acc) = data.account else {
		return request_err("Account not found.");
	};

	let amount = match parse_qty(&form.request_amount, acc.ledger_scale) {
		Ok(0) => return request_err("Enter an amount greater than zero."),
		Ok(n) => n,
		Err(e) => return request_err(&e),
	};

	let memo = {
		let t = form.request_memo.trim();
		if t.is_empty() {
			None
		} else if t.len() > MAX_MEMO_LEN {
			return request_err("Memo is too long.");
		} else {
			Some(t)
		}
	};

	let path = request_link_path(acc.ledger_id, acc.account_id, amount, memo);
	PatchSignals::json(&RequestOkPatch {
		request_link: path,
		request_error: String::new(),
		request_copied: false,
	})
}

#[derive(Debug, Deserialize)]
struct RequestSignals {
	#[serde(default, rename = "requestAmount")]
	request_amount: String,
	#[serde(default, rename = "requestMemo")]
	request_memo: String,
}

#[derive(Serialize)]
struct RequestOkPatch {
	#[serde(rename = "requestLink")]
	request_link: String,
	#[serde(rename = "requestError")]
	request_error: String,
	#[serde(rename = "requestCopied")]
	request_copied: bool,
}

#[derive(Serialize)]
struct RequestErrPatch {
	#[serde(rename = "requestError")]
	request_error: String,
	#[serde(rename = "requestLink")]
	request_link: String,
}

fn request_err(error: &str) -> Result<PatchSignals> {
	PatchSignals::json(&RequestErrPatch {
		request_error: error.to_owned(),
		request_link: String::new(),
	})
}

/// Path + query for H4. Amount is ledger base units. Memo is omitted when empty.
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
