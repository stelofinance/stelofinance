//! `GET /app/activity` — live transfer history. Live list: the `list` shard.

mod finalize;
mod list;
mod present;

use crate::auth::require_user;
use crate::domain::selected_account_id;
use crate::stdb::{acquire_user_db, fetch_activity_page};
use list::{activity_body, needle};
use topcoat::{
	Result,
	context::Cx,
	metadata::Metadata,
	router::{error::internal_server_error, page, query_params},
	runtime::signal,
	view::{View, view},
};

#[query_params]
struct ActivityQuery {
	account: Option<String>,
}

/// `GET /app/activity` — chips + day-grouped `my_transfers`. Optional `?account=`.
#[page(with(Metadata::new().title("Activity · Stelo Finance")))]
async fn page(cx: &Cx) -> Result<impl View> {
	let _user = require_user(cx).await?;
	let requested = query_params::<ActivityQuery>(cx)
		.ok()
		.and_then(|q| q.account.clone());

	let conn = acquire_user_db(cx).await?;
	let data = fetch_activity_page(conn.get())
		.await
		.map_err(internal_server_error)?;
	let selected = selected_account_id(requested.as_deref(), &data.accounts);
	let filter = signal(cx, || selected.map(needle).unwrap_or_default());
	let busy = signal(cx, || false);
	let error = signal(cx, String::new);

	Ok(view! {
    <main
        id="page-content"
        class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
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
        activity_body(filter: filter.clone(), busy: busy.clone(), error: error.clone())
        <p class="mt-4 text-sm text-red-400" :hidden=$(error.get() == "")>
            $(error.get())
        </p>
    </main>
})
}
