//! The live part of the account page: home, people and API tokens.
//!
//! One shard holding a `live!` region fed by `LiveAccountHome`. Shard and
//! procedure endpoints skip the `/app` layout guard, so each calls
//! `require_user` / `acquire_user_db`; SpacetimeDB authorizes every account
//! action as that user, and `my_accounts*` only contain accounts they belong
//! to, so an account that is not theirs renders as "no access" with no data.

use crate::auth::require_user;
use crate::domain::{format_qty, format_rel_time, parse_role, role_rank, unix_now_micros};
use crate::module_bindings::{AccountKind, MemberKind, MyAccountMemberRow, MyAccountRow, Role};
use crate::stdb::account::{
	AccountHomeData, AccountTokenView, LiveAccountHome, fetch_account_home,
	revoke_account_api_tokens, revoke_member, set_primary,
};
use crate::stdb::{LiveFeed, acquire_user_db};
use crate::ui::runtime::copy_button;
use crate::ui::text::{ledger_kind_label, role_label};
use spacetimedb_sdk::Identity;
use topcoat::{
	Result,
	context::Cx,
	runtime::{Event, Signal, connected, procedure, shard},
	view::{View, component, emit, live, view},
};

const EXPIRED: &str = "Your session expired. Reload the page.";

/// Home, people and tokens for `account_id`: SSR on the page, then live over
/// the runtime socket. The signals are only used inside `$(...)` handlers and
/// bindings, so changing them does not re-render the live region.
///
/// The three regions are `order-*` children of a `contents` wrapper so the
/// page can interleave its static forms between them.
#[shard("/app/accounts/live")]
pub async fn account_live(
	cx: &Cx,
	account_id: u64,
	account_error: Signal<String>,
	saving_primary: Signal<bool>,
	creating_token: Signal<bool>,
	has_token: Signal<bool>,
	token_label: Signal<String>,
	token_error: Signal<String>,
	revoke_token_id: Signal<u64>,
) -> Result<impl View> {
	let user = require_user(cx).await?;
	let caller_id = user.identity;
	let username = user.bitcraft_username.clone();

	Ok(live! {
    let mut feed = LiveFeed::new(move |c, tx| LiveAccountHome::start(c, tx, account_id));
    loop {
        let data = feed.next(cx).await?;
        let rendered = emit! {
            live_view(
                data: data,
                caller_id: caller_id,
                username: username.clone(),
                account_error: account_error.clone(),
                saving_primary: saving_primary.clone(),
                creating_token: creating_token.clone(),
                has_token: has_token.clone(),
                token_label: token_label.clone(),
                token_error: token_error.clone(),
                revoke_token_id: revoke_token_id.clone()
            )
        }?;
        if !connected(cx) {
            break Ok(rendered);
        }
    }
})
}

#[component]
async fn live_view(
	cx: &Cx,
	data: AccountHomeData,
	caller_id: Identity,
	username: String,
	account_error: Signal<String>,
	saving_primary: Signal<bool>,
	creating_token: Signal<bool>,
	has_token: Signal<bool>,
	token_label: Signal<String>,
	token_error: Signal<String>,
	revoke_token_id: Signal<u64>,
) -> Result<impl View> {
	Ok(view! {
    <div id="account-live" class="contents">
        <div id="account-home" class="order-1 mt-4 flex flex-col">
            home_section(
                data: &data,
                username: username,
                saving_primary: saving_primary.clone(),
                account_error: account_error.clone()
            )
        </div>
        <div id="account-people" class="order-3 flex flex-col">
            people_section(
                data: &data,
                caller_id: caller_id,
                account_error: account_error.clone()
            )
        </div>
        <div id="account-tokens" class="order-5 flex flex-col">
            tokens_section(
                data: &data,
                caller_id: caller_id,
                creating_token: creating_token.clone(),
                has_token: has_token.clone(),
                token_label: token_label.clone(),
                token_error: token_error.clone(),
                revoke_token_id: revoke_token_id.clone()
            )
        </div>
    </div>
})
}

// ---------------------------------------------------------------- home

#[component]
async fn home_section(
	cx: &Cx,
	data: &AccountHomeData,
	username: String,
	saving_primary: Signal<bool>,
	account_error: Signal<String>,
) -> Result<impl View> {
	let acc = data.account.clone();
	let has_other_primary = data.has_other_primary;
	Ok(view! {
    if let Some(acc) = acc {
        home_body(
            acc: &acc,
            has_other_primary: has_other_primary,
            username: username,
            saving_primary: saving_primary,
            account_error: account_error
        )
    } else {
        <p class="text-neutral-300">"You no longer have access to this account."</p>
        <a
            href="/app/accounts"
            class="mt-4 text-sm text-anakiwa underline-offset-2 hover:underline"
        >
            "Back to Accounts"
        </a>
    }
})
}

#[component]
async fn home_body(
	cx: &Cx,
	acc: &MyAccountRow,
	has_other_primary: bool,
	username: String,
	saving_primary: Signal<bool>,
	account_error: Signal<String>,
) -> Result<impl View> {
	let id = acc.account_id;
	let qty = format_qty(acc.balance, acc.ledger_scale);
	let label = acc
		.label
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty())
		.map(str::to_owned);
	let title = label.clone().unwrap_or_else(|| acc.ledger_name.clone());
	let debit = matches!(acc.kind, AccountKind::Debit);
	let owner = matches!(acc.role, Role::Owner);
	let can_flow = role_rank(acc.role) >= role_rank(Role::Write);
	let can_send = debit && can_flow;
	let is_primary = acc.is_primary;
	let shared = format!("Shared · {}", role_label(acc.role));
	let kind_line = ledger_kind_label(acc.ledger_kind);
	let primary_note = if is_primary {
		format!("People can send to @{username}.")
	} else {
		format!("Not primary. Sends to @{username} go elsewhere.")
	};
	let address = acc.address.clone();
	let send_href = format!("/app/transfer?from={id}");
	let deposit_href = format!("/app/deposit?from={id}");
	let withdraw_href = format!("/app/withdraw?from={id}");

	Ok(view! {
    <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
            <h1 class="text-2xl font-medium md:text-3xl">(title)</h1>
            if label.is_some() {
                <p class="mt-0.5 text-sm text-neutral-300">(acc.ledger_name.clone())</p>
            } else {
                <p class="mt-0.5 text-sm text-neutral-400">(kind_line)</p>
            }
        </div>
        <div class="flex flex-wrap gap-2">
            if is_primary {
                <span
                    class="rounded-full bg-anakiwa/15 px-2.5 py-0.5 text-xs text-anakiwa"
                >
                    "Primary"
                </span>
            }
            if !debit {
                <span
                    class="rounded-full bg-neutral-800 px-2.5 py-0.5 text-xs text-neutral-300"
                >
                    "Credit"
                </span>
            }
            if !owner {
                <span
                    class="rounded-full bg-neutral-800 px-2.5 py-0.5 text-xs text-neutral-300"
                >
                    (shared)
                </span>
            }
        </div>
    </div>
    <p class="mt-4 text-3xl font-medium tabular-nums sm:text-4xl">(qty)</p>
    <div class="mt-3 flex flex-wrap items-center gap-2 text-sm text-neutral-300">
        <span>
            "#"
            (acc.address.clone())
        </span>
        copy_button(
            text: $(address),
            class: "cursor-pointer rounded-md px-2 py-0.5 text-xs text-neutral-300 hover:bg-neutral-800 hover:text-white",
            aria_label: Some(format!("Copy address {}", acc.address))
        )
    </div>
    if debit {
        <p class="mt-3 text-sm text-neutral-300">(primary_note)</p>
    }
    if owner {
        if debit {
            <div class="mt-3">
                if is_primary {
                    primary_button(
                        text: "Turn off primary",
                        class: "cursor-pointer rounded-md border border-neutral-700 px-3 py-1.5 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white",
                        account_id: id,
                        saving: saving_primary,
                        error: account_error
                    )
                } else if has_other_primary {
                    <p class="text-sm text-neutral-400">
                        "You already have a primary account for this asset."
                    </p>
                } else {
                    primary_button(
                        text: "Make primary",
                        class: "cursor-pointer rounded-md bg-anakiwa-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-anakiwa-600",
                        account_id: id,
                        saving: saving_primary,
                        error: account_error
                    )
                }
            </div>
        }
    }
    if can_flow {
        <div class="mt-5 flex flex-wrap gap-2">
            if can_send {
                <a
                    href=(send_href)
                    class="inline-flex cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
                >
                    "Send"
                </a>
            }
            <a
                href=(deposit_href)
                class="inline-flex cursor-pointer rounded-md border border-neutral-700 px-4 py-2 text-sm text-neutral-200 hover:border-neutral-500 hover:text-white"
            >
                "Deposit"
            </a>
            <a
                href=(withdraw_href)
                class="inline-flex cursor-pointer rounded-md border border-neutral-700 px-4 py-2 text-sm text-neutral-200 hover:border-neutral-500 hover:text-white"
            >
                "Withdraw"
            </a>
        </div>
    }
})
}

#[component]
async fn primary_button(
	cx: &Cx,
	text: &'static str,
	class: &'static str,
	account_id: u64,
	saving: Signal<bool>,
	error: Signal<String>,
) -> Result<impl View> {
	Ok(view! {
    <button
        type="button"
        class=(class)
        :disabled=$(saving.get())
        @click=$(async |_e| {
            saving.set(true);
            let result = toggle_primary(account_id).await;
            saving.set(false);
            if result.is_ok() {
                error.set("".to_owned());
            } else {
                error.set(result.unwrap_err());
            }
        })
    >
        (text)
    </button>
})
}

/// Toggle the primary flag of a debit account (owner; the module enforces it).
#[procedure("/app/accounts/primary")]
async fn toggle_primary(cx: &Cx, account_id: u64) -> Result<std::result::Result<bool, String>> {
	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err(EXPIRED.into()));
	};
	let data = match fetch_account_home(conn.get(), account_id).await {
		Ok(d) => d,
		Err(e) => return Ok(Err(e.user_message().to_owned())),
	};
	let Some(acc) = data.account else {
		return Ok(Err("Account not found.".into()));
	};
	if !matches!(acc.kind, AccountKind::Debit) {
		return Ok(Err("Only debit accounts can be primary.".into()));
	}
	Ok(set_primary(conn.get(), account_id, !acc.is_primary)
		.await
		.map(|()| true)
		.map_err(|e| e.user_message().to_owned()))
}

// -------------------------------------------------------------- people

#[component]
async fn people_section(
	cx: &Cx,
	data: &AccountHomeData,
	caller_id: Identity,
	account_error: Signal<String>,
) -> Result<impl View> {
	let acc = data.account.clone();
	let members = data.members.clone();
	Ok(view! {
    if let Some(acc) = acc {
        <section class="mt-10">
            <div class="flex flex-wrap items-center justify-between gap-3">
                <h2
                    class="text-sm font-medium uppercase tracking-wide text-neutral-400"
                >
                    "Permissions"
                </h2>
            </div>
            <div class="mt-3 flex flex-col gap-2">
                #[key(m.member_id.to_hex().to_string())]
                for m in &members {
                    member_row(
                        m: m,
                        account_id: acc.account_id,
                        caller_id: caller_id,
                        caller_role: acc.role,
                        account_error: account_error.clone()
                    )
                }
            </div>
        </section>
    }
})
}

#[component]
async fn member_row(
	cx: &Cx,
	m: &MyAccountMemberRow,
	account_id: u64,
	caller_id: Identity,
	caller_role: Role,
	account_error: Signal<String>,
) -> Result<impl View> {
	let self_row = m.member_id == caller_id;
	let is_app = matches!(m.kind, MemberKind::App);
	let target_owner = matches!(m.role, Role::Owner);
	let caller_owner = matches!(caller_role, Role::Owner);
	let admin_plus = role_rank(caller_role) >= role_rank(Role::Admin);
	let controls = admin_plus && !self_row && !target_owner;
	let can_leave = self_row && !caller_owner;
	let hex = m.member_id.to_hex().to_string();
	let role_text = role_label(m.role);

	Ok(view! {
    <div
        class="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2.5"
    >
        <div class="min-w-0">
            <p class="truncate text-sm text-neutral-100">
                (m.name.clone())
                if self_row {
                    " "
                    <span class="text-neutral-400">"(you)"</span>
                }
                if is_app {
                    " "
                    <span
                        class="ml-1 rounded-full bg-neutral-800 px-2 py-0.5 text-xs text-neutral-300"
                    >
                        "App"
                    </span>
                }
            </p>
        </div>
        <div class="flex flex-wrap items-center gap-2">
            if controls {
                role_select(
                    account_id: account_id,
                    member_id: hex.clone(),
                    current: m.role,
                    allow_owner: caller_owner && !is_app,
                    error: account_error.clone()
                )
                <button
                    type="button"
                    class="cursor-pointer text-sm text-red-400 hover:text-red-300"
                    @click=$(async |_e| {
                        let result = remove_member(account_id, hex.clone()).await;
                        if result.is_ok() {
                            account_error.set("".to_owned());
                        } else {
                            account_error.set(result.unwrap_err());
                        }
                    })
                >
                    "Remove"
                </button>
            } else {
                <span class="text-sm text-neutral-300">(role_text)</span>
            }
            if can_leave {
                <button
                    type="button"
                    class="cursor-pointer text-sm text-neutral-300 hover:text-white"
                    @click=$(async |_e| {
                        let result = leave_account(account_id).await;
                        if result.is_ok() {
                            raw!("window.location.assign('/app/accounts')", {})
                        } else {
                            account_error.set(result.unwrap_err())
                        }
                    })
                >
                    "Leave"
                </button>
            }
        </div>
    </div>
})
}

#[component]
async fn role_select(
	cx: &Cx,
	account_id: u64,
	member_id: String,
	current: Role,
	allow_owner: bool,
	error: Signal<String>,
) -> Result<impl View> {
	Ok(view! {
    <select
        class="cursor-pointer rounded-md border border-neutral-800 bg-neutral-900 px-2 py-1 text-sm"
        @change=$(async |e: Event| {
            let result = change_role(
                account_id,
                member_id.clone(),
                e.target.value.to_owned(),
            ).await;
            if result.is_ok() {
                error.set("".to_owned());
            } else {
                error.set(result.unwrap_err());
            }
        })
    >
        <option value="read" selected=(matches!(current, Role::Read))>"Read"</option>
        <option value="write" selected=(matches!(current, Role::Write))>"Write"</option>
        <option value="admin" selected=(matches!(current, Role::Admin))>"Admin"</option>
        if allow_owner {
            <option value="owner" selected=(matches!(current, Role::Owner))>
                "Owner"
            </option>
        }
    </select>
})
}

/// Change an existing member's role (the select on each member row).
#[procedure("/app/accounts/members/role")]
async fn change_role(
	cx: &Cx,
	account_id: u64,
	member_id: String,
	role: String,
) -> Result<std::result::Result<bool, String>> {
	let Ok(member_id) = Identity::from_hex(member_id.trim()) else {
		return Ok(Err("Invalid member.".into()));
	};
	let Some(role) = parse_role(&role) else {
		return Ok(Err("Choose a role.".into()));
	};
	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err(EXPIRED.into()));
	};
	Ok(
		crate::stdb::account::grant_member(conn.get(), account_id, member_id, role)
			.await
			.map(|()| true)
			.map_err(|e| e.user_message().to_owned()),
	)
}

/// Remove another member from the account (admin+; the module enforces it).
#[procedure("/app/accounts/members/revoke")]
async fn remove_member(
	cx: &Cx,
	account_id: u64,
	member_id: String,
) -> Result<std::result::Result<bool, String>> {
	let Ok(member_id) = Identity::from_hex(member_id.trim()) else {
		return Ok(Err("Invalid member.".into()));
	};
	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err(EXPIRED.into()));
	};
	Ok(revoke_member(conn.get(), account_id, member_id)
		.await
		.map(|()| true)
		.map_err(|e| e.user_message().to_owned()))
}

/// Remove the caller from the account.
#[procedure("/app/accounts/leave")]
async fn leave_account(cx: &Cx, account_id: u64) -> Result<std::result::Result<bool, String>> {
	let Ok(user) = require_user(cx).await else {
		return Ok(Err(EXPIRED.into()));
	};
	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err(EXPIRED.into()));
	};
	Ok(revoke_member(conn.get(), account_id, user.identity)
		.await
		.map(|()| true)
		.map_err(|e| e.user_message().to_owned()))
}

// -------------------------------------------------------------- tokens

#[component]
async fn tokens_section(
	cx: &Cx,
	data: &AccountHomeData,
	caller_id: Identity,
	creating_token: Signal<bool>,
	has_token: Signal<bool>,
	token_label: Signal<String>,
	token_error: Signal<String>,
	revoke_token_id: Signal<u64>,
) -> Result<impl View> {
	let admin_plus = data
		.account
		.as_ref()
		.is_some_and(|a| role_rank(a.role) >= role_rank(Role::Admin));
	let account_id = data.account.as_ref().map_or(0, |a| a.account_id);
	let tokens = data.tokens.clone();
	let now = unix_now_micros();
	let is_empty = tokens.is_empty();

	Ok(view! {
    if admin_plus {
        <section class="mt-10">
            <div class="flex flex-wrap items-center justify-between gap-3">
                <h2
                    class="text-sm font-medium uppercase tracking-wide text-neutral-400"
                >
                    "API tokens"
                </h2>
                <button
                    type="button"
                    class="cursor-pointer rounded-md bg-anakiwa-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-anakiwa-600 disabled:cursor-not-allowed disabled:opacity-50"
                    :disabled=$(if creating_token.get() {
                        true
                    } else {
                        has_token.get()
                    })
                    @click=$(|_e| {
                        creating_token.set(true);
                        token_error.set("".to_owned());
                        token_label.set("".to_owned());
                        revoke_token_id.set(0u64);
                    })
                >
                    "New token"
                </button>
            </div>
            <p class="mt-2 text-sm text-neutral-400">
                "Scripts and bots use these with the JSON API."
            </p>
            if is_empty {
                <div
                    class="mt-3 rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-6 text-center text-sm text-neutral-400"
                >
                    "No tokens yet."
                </div>
            } else {
                <div class="mt-3 flex flex-col gap-2">
                    #[key(t.id)]
                    for t in &tokens {
                        token_card(
                            token: t,
                            account_id: account_id,
                            caller_id: caller_id,
                            now: now,
                            token_error: token_error.clone(),
                            revoke_token_id: revoke_token_id.clone()
                        )
                    }
                </div>
            }
            <p
                class="mt-3 text-sm text-red-400"
                :hidden=$(if token_error.get().is_empty() {
                    true
                } else if creating_token.get() {
                    true
                } else {
                    has_token.get()
                })
            >
                $(token_error.get())
            </p>
        </section>
    }
})
}

#[component]
async fn token_card(
	cx: &Cx,
	token: &AccountTokenView,
	account_id: u64,
	caller_id: Identity,
	now: i64,
	token_error: Signal<String>,
	revoke_token_id: Signal<u64>,
) -> Result<impl View> {
	let trimmed = token.label.trim();
	let untitled = trimmed.is_empty();
	let title = if untitled { "(untitled)" } else { trimmed }.to_owned();
	let when = format_rel_time(token.created_micros, now);
	let who = token_minter_label(token, caller_id);
	let id = token.id;
	let prompt = format!("Revoke “{title}”? Integrations using it will fail.");

	Ok(view! {
    <div class="rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2.5">
        <div class="flex flex-wrap items-center justify-between gap-2">
            if untitled {
                <p class="truncate text-sm text-neutral-400">(title.clone())</p>
            } else {
                <p class="truncate text-sm text-neutral-100">(title.clone())</p>
            }
            <button
                type="button"
                class="cursor-pointer text-sm text-red-400 hover:text-red-300"
                @click=$(|_e| {
                    revoke_token_id.set(id);
                    token_error.set("".to_owned());
                })
            >
                "Revoke"
            </button>
        </div>
        <p class="mt-1 text-sm text-neutral-400">
            (when)
            " · "
            (who)
        </p>
        <div
            class="mt-3 border-t border-neutral-800 pt-3"
            :hidden=$(revoke_token_id.get() != id)
        >
            <p class="text-sm text-neutral-300">(prompt)</p>
            <div class="mt-3 flex flex-wrap gap-2">
                <button
                    type="button"
                    class="cursor-pointer rounded-md border border-neutral-700 px-3 py-1.5 text-sm text-neutral-300 hover:border-neutral-500 hover:text-white"
                    @click=$(|_e| revoke_token_id.set(0u64))
                >
                    "Cancel"
                </button>
                <button
                    type="button"
                    class="cursor-pointer rounded-md bg-red-900 px-3 py-1.5 text-sm font-medium text-red-100 hover:bg-red-800"
                    @click=$(async |_e| {
                        let result = revoke_token(account_id, id).await;
                        if result.is_ok() {
                            token_error.set("".to_owned());
                            revoke_token_id.set(0u64);
                        } else {
                            token_error.set(result.unwrap_err());
                        }
                    })
                >
                    "Revoke"
                </button>
            </div>
        </div>
    </div>
})
}

fn token_minter_label(token: &AccountTokenView, caller_id: Identity) -> String {
	if token.created_by == caller_id {
		return "you".to_owned();
	}
	if let Some(name) = token
		.created_by_username
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty())
	{
		return name.to_owned();
	}
	let hex = token.created_by.to_hex();
	hex.get(..8).unwrap_or(hex.as_str()).to_owned()
}

/// Revoke one API token (admin+; the module enforces it).
#[procedure("/app/accounts/tokens/revoke")]
async fn revoke_token(
	cx: &Cx,
	account_id: u64,
	token_id: u64,
) -> Result<std::result::Result<bool, String>> {
	if token_id == 0 {
		return Ok(Err("Pick a token to revoke.".into()));
	}
	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err(EXPIRED.into()));
	};
	Ok(
		revoke_account_api_tokens(conn.get(), account_id, vec![token_id])
			.await
			.map(|()| true)
			.map_err(|e| e.user_message().to_owned()),
	)
}
