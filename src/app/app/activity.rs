//! `GET /app/activity` — H3b live transfer history. Live patches: `updates`.

mod list;
mod present;
mod updates;

use crate::auth::require_user;
use crate::stdb::{
	StdbError, acquire_user_db, fetch_activity_page, selected_account_id, unix_now_micros,
};
use list::activity_body;
use topcoat::{
	Result,
	context::Cx,
	router::{error::internal_server_error, page, query_params},
	view::view,
};

#[query_params]
struct ActivityQuery {
	account: Option<String>,
}

/// `GET /app/activity` — chips + day-grouped `my_transfers`. Optional `?account=`.
#[page]
async fn page(cx: &Cx) -> Result {
	let _user = require_user(cx).await?;
	let requested = query_params::<ActivityQuery>(cx)
		.ok()
		.and_then(|q| q.account.clone());

	let conn = acquire_user_db(cx).await?;
	let data = fetch_activity_page(conn.get())
		.await
		.map_err(|e| internal_server_error(StdbError(e)))?;
	let selected = selected_account_id(requested.as_deref(), &data.accounts);
	let account_id = selected.map(|id| id.to_string()).unwrap_or_default();
	let signals = format!("{{accountId:'{account_id}'}}");
	let now = unix_now_micros();

	view! {
		<main
			id="page-content"
			class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
			data-signals=(signals)
			data-init="@get('/app/activity/updates')"
		>
			<div class="mb-6 flex items-center justify-between gap-3">
				<h1 class="text-2xl font-medium md:text-3xl">"Activity"</h1>
				<a
					href="/app/transfer"
					class="rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
				>
					"Send"
				</a>
			</div>
			activity_body(data: data, selected: selected, now_micros: now)
		</main>
	}
}
