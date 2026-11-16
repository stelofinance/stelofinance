//! Add-permission panel on the account page (Topcoat runtime trial).
//!
//! Panel state lives in runtime signals; the user and app searches are
//! shards; granting is a typed procedure. Shard and procedure endpoints skip
//! the `/app` layout guard: each calls `acquire_user_db`, which requires a
//! session, and SpacetimeDB authorizes the account action as that user.

use crate::domain::parse_role;
use crate::stdb::account::{grant_member, search_users};
use crate::stdb::{acquire_user_db, search_apps};
use crate::ui::runtime::search_input;
use spacetimedb_sdk::Identity;
use topcoat::{
	Result,
	context::Cx,
	router::error::internal_server_error,
	runtime::{Event, Signal, procedure, shard, signal},
	view::{View, component, view},
};

const TAB_ON: &str = "cursor-pointer rounded-md bg-anakiwa-700 px-3 py-1.5 text-sm text-white";
const TAB_OFF: &str =
	"cursor-pointer rounded-md bg-neutral-800 px-3 py-1.5 text-sm text-neutral-300";
const INPUT: &str = "mt-3 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm placeholder:text-neutral-400";

/// Grant `member_id` (identity hex) `role` on `account_id`.
///
/// Failures come back as `Err(message)` data: a procedure `Err` only fails
/// the browser expression, which could not reset the busy state. `Ok` is a
/// `bool` because `Result<(), String>` does not serialize as a runtime value.
#[procedure("/app/accounts/members/grant")]
async fn grant_permission(
	cx: &Cx,
	account_id: u64,
	member_id: String,
	role: String,
) -> Result<std::result::Result<bool, String>> {
	let Ok(member_id) = Identity::from_hex(member_id.trim()) else {
		return Ok(Err("Pick a person or app from the search results.".into()));
	};
	let Some(role) = parse_role(&role) else {
		return Ok(Err("Choose a role.".into()));
	};
	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err("Your session expired. Reload the page.".into()));
	};
	Ok(grant_member(conn.get(), account_id, member_id, role)
		.await
		.map(|()| true)
		.map_err(|e| e.user_message().to_owned()))
}

/// Username search results. Picking one fills the panel's member signals.
#[shard("/app/accounts/members/user-results")]
async fn user_results(
	cx: &Cx,
	query: String,
	member_id: Signal<String>,
	member_name: Signal<String>,
	search: Signal<String>,
) -> Result<impl View> {
	let term = query.trim();
	let hits = if term.is_empty() {
		None
	} else {
		let conn = acquire_user_db(cx).await?;
		let me = crate::auth::require_user(cx).await?.identity;
		let rows = search_users(conn.get(), term, &[me])
			.await
			.map_err(internal_server_error)?;
		Some(
			rows.into_iter()
				.map(|r| {
					(
						r.id.to_hex().to_string(),
						format!("@{}", r.bitcraft_username),
						String::new(),
					)
				})
				.collect(),
		)
	};
	Ok(view! {
    results(
        hits: hits,
        empty: "No users match that name.",
        member_id: member_id,
        member_name: member_name,
        search: search
    )
})
}

/// App name search results. Picking one fills the panel's member signals.
#[shard("/app/accounts/members/app-results")]
async fn app_results(
	cx: &Cx,
	query: String,
	member_id: Signal<String>,
	member_name: Signal<String>,
	search: Signal<String>,
) -> Result<impl View> {
	let term = query.trim();
	let hits = if term.is_empty() {
		None
	} else {
		let conn = acquire_user_db(cx).await?;
		let rows = search_apps(conn.get(), term)
			.await
			.map_err(internal_server_error)?;
		Some(
			rows.into_iter()
				.map(|r| {
					let owner = r
						.owner_username
						.as_deref()
						.map(str::trim)
						.filter(|s| !s.is_empty())
						.map(|u| format!(" · {u}"))
						.unwrap_or_default();
					(r.id.to_hex().to_string(), r.name, owner)
				})
				.collect(),
		)
	};
	Ok(view! {
    results(
        hits: hits,
        empty: "No apps match that name.",
        member_id: member_id,
        member_name: member_name,
        search: search
    )
})
}

/// `(identity hex, display name, muted suffix)` rows; `None` before a search.
#[component]
async fn results(
	cx: &Cx,
	hits: Option<Vec<(String, String, String)>>,
	empty: &'static str,
	member_id: Signal<String>,
	member_name: Signal<String>,
	search: Signal<String>,
) -> Result<impl View> {
	Ok(view! {
    if let Some(hits) = hits {
        <div
            class="mt-2 flex flex-col overflow-hidden rounded-md border border-neutral-800 bg-neutral-900"
        >
            if hits.is_empty() {
                <p class="px-3 py-2 text-sm text-neutral-400">(empty)</p>
            }
            for (hex, name, suffix) in hits {
                <button
                    type="button"
                    class="cursor-pointer px-3 py-2 text-left text-sm text-neutral-200 hover:bg-neutral-800"
                    @click=$(|_e| {
                        member_id.set(hex.clone());
                        member_name.set(name.clone());
                        search.set("".to_owned());
                    })
                >
                    (name.clone())
                    <span class="text-neutral-500">(suffix)</span>
                </button>
            }
        </div>
    }
})
}

/// The "Add permission" toggle and panel. Rendered for admin+ only; the
/// module enforces that again on grant.
#[component]
pub async fn permission_panel(cx: &Cx, account_id: u64) -> Result<impl View> {
	let open = signal(cx, || false);
	let is_app = signal(cx, || false);
	let user_search = signal(cx, String::new);
	let app_search = signal(cx, String::new);
	let member_id = signal(cx, String::new);
	let member_name = signal(cx, String::new);
	let role = signal(cx, || "write".to_owned());
	let busy = signal(cx, || false);
	let error = signal(cx, String::new);

	Ok(view! {
    <div class="mt-3 flex justify-end" :hidden=$(open.get())>
        <button
            type="button"
            class="cursor-pointer rounded-md bg-anakiwa-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-anakiwa-600"
            @click=$(|_e| {
                open.set(true);
                is_app.set(false);
                member_id.set("".to_owned());
                member_name.set("".to_owned());
                user_search.set("".to_owned());
                app_search.set("".to_owned());
                role.set("write".to_owned());
                error.set("".to_owned());
            })
        >
            "Add permission"
        </button>
    </div>
    <section
        class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
        :hidden=$(!open.get())
    >
        <h2 class="text-lg font-medium">"Add permission"</h2>
        <p class="mt-1 text-sm text-neutral-300">
            "Grant a person or an app a role on this account. Create apps on your profile."
        </p>
        <div class="mt-4 flex gap-2">
            <button
                type="button"
                :class=$(if is_app.get() { TAB_OFF } else { TAB_ON })
                :aria-pressed=$(if is_app.get() { "false" } else { "true" })
                @click=$(|_e| {
                    is_app.set(false);
                    member_id.set("".to_owned());
                    member_name.set("".to_owned());
                    app_search.set("".to_owned());
                })
            >
                "Person"
            </button>
            <button
                type="button"
                :class=$(if is_app.get() { TAB_ON } else { TAB_OFF })
                :aria-pressed=$(if is_app.get() { "true" } else { "false" })
                @click=$(|_e| {
                    is_app.set(true);
                    member_id.set("".to_owned());
                    member_name.set("".to_owned());
                    user_search.set("".to_owned());
                })
            >
                "App"
            </button>
        </div>
        <div :hidden=$(!member_id.get().is_empty())>
            <div :hidden=$(is_app.get())>
                search_input(
                    query: user_search.clone(),
                    class: INPUT,
                    placeholder: $("Search username…".to_owned())
                )
                user_results(
                    query: $(user_search.get()),
                    member_id: member_id.clone(),
                    member_name: member_name.clone(),
                    search: user_search.clone()
                )
            </div>
            <div :hidden=$(!is_app.get())>
                search_input(
                    query: app_search.clone(),
                    class: INPUT,
                    placeholder: $("Search app name…".to_owned())
                )
                app_results(
                    query: $(app_search.get()),
                    member_id: member_id.clone(),
                    member_name: member_name.clone(),
                    search: app_search.clone()
                )
            </div>
        </div>
        <div
            class="mt-3 flex items-center justify-between rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2"
            :hidden=$(member_id.get().is_empty())
        >
            <p class="truncate text-sm">$(member_name.get())</p>
            <button
                type="button"
                class="cursor-pointer text-sm text-neutral-300 hover:text-white"
                @click=$(|_e| {
                    member_id.set("".to_owned());
                    member_name.set("".to_owned());
                })
            >
                "Clear"
            </button>
        </div>
        <div class="mt-4 flex flex-wrap gap-2">
            <select
                class="cursor-pointer rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm"
                :value=$(role.get())
                @change=$(|e: Event| role.set(e.target.value))
            >
                <option value="read">"Read"</option>
                <option value="write" selected="">"Write"</option>
                <option value="admin">"Admin"</option>
            </select>
            <button
                type="button"
                class="cursor-pointer rounded-md border border-neutral-700 px-3 py-1.5 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white"
                @click=$(|_e| {
                    open.set(false);
                    error.set("".to_owned());
                })
            >
                "Cancel"
            </button>
            <button
                type="button"
                class="cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
                :disabled=$(if busy.get() { true } else { member_id.get().is_empty() })
                @click=$(async |_e| {
                    busy.set(true);
                    let result = grant_permission(
                        account_id,
                        member_id.get(),
                        role.get(),
                    ).await;
                    busy.set(false);
                    if result.is_ok() {
                        open.set(false);
                        member_id.set("".to_owned());
                        member_name.set("".to_owned());
                        error.set("".to_owned());
                    } else {
                        error.set(result.unwrap_err());
                    }
                })
            >
                $(if busy.get() { "Adding…" } else { "Add" })
            </button>
        </div>
        <p class="mt-3 text-sm text-red-400" :hidden=$(error.get().is_empty())>
            $(error.get())
        </p>
    </section>
})
}
