//! Live apps list shard, token sheet, and the `token` procedure.

use crate::auth::require_user;
use crate::domain::{format_rel_time, unix_now_micros};
use crate::module_bindings::{AppTicketPurpose, MyAppRow, MyAppTicketRow};
use crate::stdb::apps::{LiveMyApps, MyAppsData, mint_app_identity};
use crate::stdb::{
	LiveFeed, StdbState, acquire_user_db, create_user_app_ticket, replace_user_app_ticket,
};
use crate::ui::runtime::copy_button;
use topcoat::{
	Result,
	context::{Cx, context},
	runtime::{Event, Expr, Signal, connected, procedure, record, shard},
	view::{View, component, emit, live, view},
};

/// The page's signals, only used inside `$(...)` by the live list so that
/// changing them does not re-render it. It gets `mint_id`, never `token`:
/// signal values are sent with every shard request, and the secret must stay
/// in the browser.
///
/// `#me-apps`: SSR on the page, then live over the runtime socket. Shard
/// endpoints skip the `/app` layout guard, so this checks the session itself.
#[shard("/app/me/apps")]
pub async fn apps_list(
	cx: &Cx,
	creating: Signal<bool>,
	replacing: Signal<bool>,
	mint_id: Signal<String>,
	name: Signal<String>,
	error: Signal<String>,
) -> Result<impl View> {
	require_user(cx).await?;

	Ok(live! {
    let mut feed = LiveFeed::new(LiveMyApps::start);
    loop {
        let data = feed.next(cx).await?;
        let rendered = emit! {
            apps_view(
                data: data,
                creating: creating.clone(),
                replacing: replacing.clone(),
                mint_id: mint_id.clone(),
                name: name.clone(),
                error: error.clone()
            )
        }?;
        if !connected(cx) {
            break Ok(rendered);
        }
    }
})
}

#[component]
async fn apps_view(
	cx: &Cx,
	data: MyAppsData,
	creating: Signal<bool>,
	replacing: Signal<bool>,
	mint_id: Signal<String>,
	name: Signal<String>,
	error: Signal<String>,
) -> Result<impl View> {
	let is_empty = data.apps.is_empty() && data.tickets.is_empty();
	let now = unix_now_micros();

	Ok(view! {
    <div id="me-apps" class="flex flex-col">
        <section class="mt-8">
            <div class="flex flex-wrap items-center justify-between gap-3">
                <h2
                    class="text-sm font-medium uppercase tracking-wide text-neutral-400"
                >
                    "Apps"
                </h2>
                <button
                    type="button"
                    class="cursor-pointer rounded-md bg-anakiwa-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
                    :disabled=$(if creating.get() {
                        true
                    } else if replacing.get() {
                        true
                    } else {
                        mint_id.get() != ""
                    })
                    @click=$(|_e| {
                        creating.set(true);
                        replacing.set(false);
                        error.set("".to_owned());
                        name.set("".to_owned());
                    })
                >
                    "New app"
                </button>
            </div>
            <p class="mt-2 text-sm text-neutral-400">
                "Bots you own. Copy the token once, then grant a role on an account under Permissions."
            </p>
            if is_empty {
                <p class="mt-3 text-sm text-neutral-400">
                    "No apps yet. Use "
                    <span class="text-neutral-200">"New app"</span>
                    " to generate a token."
                </p>
            }
            if !data.tickets.is_empty() {
                <p
                    class="mt-4 text-xs font-medium uppercase tracking-wide text-neutral-500"
                >
                    "Open tickets"
                </p>
                <div class="mt-2 flex flex-col gap-2">
                    #[key(t.id)]
                    for t in &data.tickets {
                        ticket_row(ticket: t, now: now)
                    }
                </div>
            }
            if !data.apps.is_empty() {
                <div class="mt-3 flex flex-col gap-2">
                    #[key(a.id.to_hex().to_string())]
                    for a in &data.apps {
                        app_row(
                            app: a,
                            creating: creating.clone(),
                            replacing: replacing.clone(),
                            mint_id: mint_id.clone(),
                            name: name.clone(),
                            error: error.clone()
                        )
                    }
                </div>
            }
        </section>
    </div>
})
}

#[component]
async fn ticket_row(cx: &Cx, ticket: &MyAppTicketRow, now: i64) -> Result<impl View> {
	let purpose = match ticket.purpose {
		AppTicketPurpose::Create => "Create",
		AppTicketPurpose::Replace => "Replace",
	};
	let left = format_rel_time(ticket.expires_at.to_micros_since_unix_epoch(), now);
	Ok(view! {
    <div class="rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2.5">
        <p class="truncate text-sm text-neutral-100">(ticket.name.clone())</p>
        <p class="mt-0.5 text-xs text-neutral-400">
            (purpose)
            " · expires "
            (left)
        </p>
    </div>
})
}

#[component]
async fn app_row(
	cx: &Cx,
	app: &MyAppRow,
	creating: Signal<bool>,
	replacing: Signal<bool>,
	mint_id: Signal<String>,
	name: Signal<String>,
	error: Signal<String>,
) -> Result<impl View> {
	let app_name = app.name.clone();
	Ok(view! {
    <div
        class="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2.5"
    >
        <div class="min-w-0">
            <p class="truncate text-sm text-neutral-100">(app.name.clone())</p>
            <p class="mt-0.5 truncate font-mono text-xs text-neutral-500">
                (app.id.to_hex().to_string())
            </p>
        </div>
        <button
            type="button"
            class="cursor-pointer text-sm text-neutral-300 hover:text-white disabled:cursor-not-allowed disabled:opacity-50"
            :disabled=$(if creating.get() {
                true
            } else if replacing.get() {
                true
            } else {
                mint_id.get() != ""
            })
            @click=$(|_e| {
                replacing.set(true);
                creating.set(false);
                error.set("".to_owned());
                name.set(app_name.clone());
            })
        >
            "Replace"
        </button>
    </div>
})
}

/// New-app form, replace form and one-time token sheet (outside the live list).
#[component]
pub async fn app_forms(
	cx: &Cx,
	creating: Signal<bool>,
	replacing: Signal<bool>,
	token: Signal<String>,
	mint_name: Signal<String>,
	mint_id: Signal<String>,
	name: Signal<String>,
	error: Signal<String>,
	minting: Signal<bool>,
) -> Result<impl View> {
	Ok(view! {
    <section
        class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
        :hidden=$(if creating.get() { token.get() != "" } else { true })
    >
        <h2 class="text-lg font-medium">"New app"</h2>
        <p class="mt-1 text-sm text-neutral-300">
            "Generates a SpacetimeDB identity and token for your bot. Copy the token once. Then grant the app a role on an account."
        </p>
        <input
            type="text"
            class="mt-4 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm placeholder:text-neutral-400"
            :value=$(name.get())
            @input=$(|e: Event| name.set(e.target.value))
            maxlength="64"
            placeholder="App name"
            autocomplete="off"
        >
        <div class="mt-4 flex flex-wrap gap-2">
            <button
                type="button"
                class="cursor-pointer rounded-md border border-neutral-700 px-3 py-1.5 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white"
                @click=$(|_e| {
                    creating.set(false);
                    name.set("".to_owned());
                    error.set("".to_owned());
                })
            >
                "Cancel"
            </button>
            generate_button(
                disabled: $(if minting.get() { true } else { name.get().trim() == "" }),
                minting: minting.clone(),
                name: name.clone(),
                replacing: replacing.clone(),
                token: token.clone(),
                mint_name: mint_name.clone(),
                mint_id: mint_id.clone(),
                error: error.clone()
            )
        </div>
    </section>
    <section
        class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
        :hidden=$(if replacing.get() { token.get() != "" } else { true })
    >
        <h2 class="text-lg font-medium">"Replace app identity"</h2>
        <p class="mt-1 text-sm text-neutral-300">
            "Generates a new SpacetimeDB identity and token for this app. The name stays the same; existing account grants move to the new identity. Copy the new token once."
        </p>
        <p class="mt-4 truncate text-sm text-neutral-100">
            <span class="text-neutral-500">"Name "</span>
            <span>$(name.get())</span>
        </p>
        <div class="mt-4 flex flex-wrap gap-2">
            <button
                type="button"
                class="cursor-pointer rounded-md border border-neutral-700 px-3 py-1.5 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white"
                @click=$(|_e| {
                    replacing.set(false);
                    name.set("".to_owned());
                    error.set("".to_owned());
                })
            >
                "Cancel"
            </button>
            generate_button(
                disabled: $(minting.get()),
                minting: minting.clone(),
                name: name.clone(),
                replacing: replacing.clone(),
                token: token.clone(),
                mint_name: mint_name.clone(),
                mint_id: mint_id.clone(),
                error: error.clone()
            )
        </div>
    </section>
    <section
        class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
        :hidden=$(token.get() == "")
    >
        <h2 class="text-lg font-medium">"App token"</h2>
        <p class="mt-1 text-sm text-neutral-300">
            "Copy this now. The bot uses it to connect. We won't show it again."
        </p>
        <p
            class="mt-2 truncate text-sm text-neutral-400"
            :hidden=$(mint_name.get() == "")
        >
            <span class="text-neutral-500">"Name "</span>
            <span>$(mint_name.get())</span>
        </p>
        <p
            class="mt-1 truncate font-mono text-xs text-neutral-500"
            :hidden=$(mint_id.get() == "")
        >
            <span>$(mint_id.get())</span>
        </p>
        <p class="mt-4 text-xs font-medium uppercase tracking-wide text-neutral-500">
            "Token"
        </p>
        <div
            class="mt-1 flex items-stretch overflow-hidden rounded-lg border border-neutral-800"
        >
            <code
                class="min-w-0 flex-1 truncate bg-neutral-900 px-3 py-2 text-sm text-anakiwa"
            >
                $(token.get())
            </code>
            copy_button(
                text: $(token.get()),
                class: "shrink-0 cursor-pointer border-l border-neutral-800 px-3 text-xs text-neutral-300 hover:bg-neutral-900 hover:text-white"
            )
        </div>
        <button
            type="button"
            class="mt-4 cursor-pointer rounded-md bg-neutral-800 px-4 py-2 text-sm hover:bg-neutral-700"
            @click=$(|_e| {
                token.set("".to_owned());
                mint_name.set("".to_owned());
                mint_id.set("".to_owned());
                name.set("".to_owned());
                creating.set(false);
                replacing.set(false);
            })
        >
            "Done"
        </button>
    </section>
})
}

/// "Generate Token" button shared by the new and replace forms.
#[component]
async fn generate_button(
	cx: &Cx,
	disabled: Expr<bool>,
	minting: Signal<bool>,
	name: Signal<String>,
	replacing: Signal<bool>,
	token: Signal<String>,
	mint_name: Signal<String>,
	mint_id: Signal<String>,
	error: Signal<String>,
) -> Result<impl View> {
	Ok(view! {
    <button
        type="button"
        class="cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
        :disabled=(disabled)
        @click=$(async |_e| {
            error.set("".to_owned());
            minting.set(true);
            let result = mint_app_token(name.get(), replacing.get()).await;
            minting.set(false);
            if result.is_ok() {
                let minted = result.unwrap();
                token.set(minted.token);
                mint_name.set(minted.name);
                mint_id.set(minted.id);
            } else {
                error.set(result.unwrap_err());
            }
        })
    >
        "Generate Token"
    </button>
})
}

/// The one-time secret. It is returned to the browser and kept only in a signal.
#[record]
#[derive(Clone, Debug)]
struct Minted {
	token: String,
	name: String,
	id: String,
}

/// `POST /app/me/apps/token` — mint a host-signed identity, reserve it with a ticket, and
/// return the token to the browser only.
#[procedure("/app/me/apps/token")]
async fn mint_app_token(
	cx: &Cx,
	name: String,
	replacing: bool,
) -> Result<std::result::Result<Minted, String>> {
	if require_user(cx).await.is_err() {
		return Ok(Err("Your session expired. Reload the page.".into()));
	}
	let name = name.trim().to_owned();
	if name.is_empty() || name.len() > 64 {
		return Ok(Err("Enter an app name (max 64 characters).".into()));
	}

	let stdb = context::<StdbState>(cx);
	let minted = match mint_app_identity(stdb).await {
		Ok(m) => m,
		Err(e) => {
			eprintln!("app token: {e}");
			return Ok(Err("Couldn't mint a token from SpacetimeDB.".into()));
		}
	};

	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err("Your session expired. Reload the page.".into()));
	};
	let ticket = if replacing {
		replace_user_app_ticket(conn.get(), name.clone(), minted.identity.clone()).await
	} else {
		create_user_app_ticket(conn.get(), name.clone(), minted.identity.clone()).await
	};
	if let Err(e) = ticket {
		return Ok(Err(e.user_message().to_owned()));
	}

	// Connect once as the app so the ticket is fulfilled right away rather than
	// waiting on the bot's first connection (tickets expire after 15 minutes).
	// If this fails the token isn't usable, so don't hand it out.
	match stdb.pool.acquire(&minted.token).await {
		Ok(_) => stdb.pool.invalidate_token(&minted.token),
		Err(e) => {
			eprintln!("app token: fulfill connect: {e}");
			return Ok(Err(
				"Couldn't register the app with SpacetimeDB. Try again.".into(),
			));
		}
	}

	Ok(Ok(Minted {
		token: minted.token,
		name,
		id: minted.identity,
	}))
}
