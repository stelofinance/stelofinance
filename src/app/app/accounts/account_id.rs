//! `GET /app/accounts/{account_id}` — account home.

mod markup;
mod permission;
mod request;
mod tokens;
mod updates;
mod webhook;

use crate::auth::require_user;
use crate::domain::{parse_role, role_rank};
use crate::module_bindings::{AccountKind, Role};
use crate::stdb::acquire_user_db;
use crate::stdb::{
	account::fetch_account_home, account::grant_member, account::revoke_member, account::set_label,
	account::set_primary,
};
use crate::ui::text::js_str;
use markup::{HomeChrome, account_home, account_people, account_tokens};
use permission::permission_panel;
use request::request_form;
use serde::{Deserialize, Serialize};
use spacetimedb_sdk::Identity;
use tokens::token_forms;
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchSignals, Signals},
	metadata::Metadata,
	router::{
		error::{internal_server_error, not_found},
		module_param, page, route,
	},
	view::{View, component, view},
};
use webhook::webhook_form;

// `module_param!` makes this module's URL segment `{account_id}` and declares
// the `account_id(cx)` accessor (also used by the absolute `{account_id}` routes).
module_param!(pub account_id: u64 else not_found);

#[page(with(Metadata::new().title("Account · Stelo Finance")))]
async fn show(cx: &Cx) -> Result<impl View> {
	let user = require_user(cx).await?;
	let account_id = *account_id(cx)?;
	let conn = acquire_user_db(cx).await?;
	let data = fetch_account_home(conn.get(), account_id)
		.await
		.map_err(internal_server_error)?;
	let Some(acc) = &data.account else {
		return Err(not_found().into());
	};

	let admin_plus = role_rank(acc.role) >= role_rank(Role::Admin);
	let label_value = acc.label.clone().unwrap_or_default();
	let webhook_value = acc.webhook.clone().unwrap_or_default();
	let signals = format!(
		"{{copiedId:0,label:{},editMemberId:'',editRole:'',revokeId:'',accountError:'',leftAccount:false,requestAmount:'',requestMemo:'',requestLink:'',requestError:'',requestCopied:false,creatingToken:false,tokenLabel:'',newToken:'',tokenError:'',tokenCopied:false,revokeTokenId:0,revokeTokenLabel:'',webhookUrl:{},webhookError:'',webhookNotice:''}}",
		js_str(&label_value),
		js_str(&webhook_value),
	);
	let debit = matches!(acc.kind, AccountKind::Debit);
	let ledger_name = acc.ledger_name.clone();
	let updates = format!("@get('/app/accounts/{account_id}/updates', {{retry: 'always'}})");
	let label_url = format!("/app/accounts/{account_id}/label");
	let chrome = HomeChrome {
		caller_id: user.identity,
		caller_username: user.bitcraft_username.clone(),
	};

	Ok(view! {
    <main
        id="page-content"
        class="mx-auto flex w-full max-w-3xl flex-col px-3 py-6 text-white sm:px-5 md:px-8 md:py-10"
        data-signals=(signals)
        data-init=(updates)
        data-effect="$leftAccount && (window.location.href = '/app/accounts')"
    >
        <a href="/app/accounts" class="text-sm text-neutral-300 hover:text-white">
            "← Accounts"
        </a>
        account_home(data: data.clone(), chrome: chrome.clone())
        request_form(account_id: account_id, ledger_name: ledger_name, debit: debit)
        account_people(data: data.clone(), chrome: chrome.clone())
        if admin_plus {
            permission_panel(account_id: account_id)
            admin_forms(label_url: label_url)
        }
        account_tokens(data: data, chrome: chrome)
        if admin_plus {
            token_forms(account_id: account_id)
            webhook_form(account_id: account_id)
        }
        <p
            class="mt-4 text-sm text-red-400"
            data-show="$accountError"
            data-text="$accountError"
        ></p>
    </main>
})
}

#[component]
async fn admin_forms(cx: &Cx, label_url: String) -> Result<impl View> {
	Ok(view! {
    <section
        class="mt-8 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5"
    >
        <h2 class="text-sm font-medium uppercase tracking-wide text-neutral-400">
            "Label"
        </h2>
        <p class="mt-1 text-sm text-neutral-400">
            "A nickname only people on this account can see."
        </p>
        <div class="mt-3 flex flex-col gap-2 sm:flex-row">
            <input
                type="text"
                class="w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm placeholder:text-neutral-400"
                data-bind="label"
                maxlength="32"
                placeholder="e.g. Guild treasury"
                autocomplete="off"
            >
            <button
                type="button"
                class="cursor-pointer rounded-md bg-neutral-800 px-4 py-2 text-sm hover:bg-neutral-700"
                data-on:click=(format!("@post('{label_url}')"))
                data-indicator="savingLabel"
            >
                "Save"
            </button>
        </div>
    </section>
})
}

#[route(POST "/app/accounts/{account_id}/primary")]
async fn set_primary_route(cx: &Cx) -> Result<PatchSignals> {
	let account_id = *account_id(cx)?;
	let conn = acquire_user_db(cx).await?;
	let data = match fetch_account_home(conn.get(), account_id).await {
		Ok(d) => d,
		Err(e) => return err_signal(e.user_message()),
	};
	let Some(acc) = data.account else {
		return err_signal("Account not found.");
	};
	if !matches!(acc.kind, AccountKind::Debit) {
		return err_signal("Only debit accounts can be primary.");
	}
	match set_primary(conn.get(), account_id, !acc.is_primary).await {
		Ok(()) => clear_error(),
		Err(e) => err_signal(e.user_message()),
	}
}

#[route(POST "/app/accounts/{account_id}/label")]
async fn save_label(cx: &Cx, Signals(form): Signals<LabelSignals>) -> Result<PatchSignals> {
	let account_id = *account_id(cx)?;
	let conn = acquire_user_db(cx).await?;
	let trimmed = form.label.trim();
	let label = if trimmed.is_empty() {
		None
	} else {
		Some(trimmed.to_owned())
	};
	match set_label(conn.get(), account_id, label).await {
		Ok(()) => clear_error(),
		Err(e) => err_signal(e.user_message()),
	}
}

/// Change an existing member's role (the select on each member row).
#[route(POST "/app/accounts/{account_id}/members")]
async fn add_member(cx: &Cx, Signals(form): Signals<MemberSignals>) -> Result<PatchSignals> {
	let account_id = *account_id(cx)?;
	let Ok(member_id) = Identity::from_hex(form.edit_member_id.trim()) else {
		return err_signal("Invalid member.");
	};
	let Some(role) = parse_role(&form.edit_role) else {
		return err_signal("Choose a role.");
	};
	let conn = acquire_user_db(cx).await?;
	match grant_member(conn.get(), account_id, member_id, role).await {
		Ok(()) => clear_error(),
		Err(e) => err_signal(e.user_message()),
	}
}

#[route(POST "/app/accounts/{account_id}/revoke")]
async fn revoke_member_route(
	cx: &Cx,
	Signals(form): Signals<RevokeSignals>,
) -> Result<PatchSignals> {
	let account_id = *account_id(cx)?;
	let Ok(member_id) = Identity::from_hex(form.revoke_id.trim()) else {
		return err_signal("Invalid member.");
	};
	let conn = acquire_user_db(cx).await?;
	match revoke_member(conn.get(), account_id, member_id).await {
		Ok(()) => clear_error(),
		Err(e) => err_signal(e.user_message()),
	}
}

#[route(POST "/app/accounts/{account_id}/leave")]
async fn leave_account(cx: &Cx) -> Result<PatchSignals> {
	let user = require_user(cx).await?;
	let account_id = *account_id(cx)?;
	let conn = acquire_user_db(cx).await?;
	match revoke_member(conn.get(), account_id, user.identity).await {
		Ok(()) => PatchSignals::json(&AccountPatch {
			account_error: String::new(),
			left_account: true,
		}),
		Err(e) => err_signal(e.user_message()),
	}
}

#[derive(Debug, Deserialize)]
struct LabelSignals {
	#[serde(default)]
	label: String,
}

#[derive(Debug, Deserialize)]
struct MemberSignals {
	#[serde(default, rename = "editMemberId")]
	edit_member_id: String,
	#[serde(default, rename = "editRole")]
	edit_role: String,
}

#[derive(Debug, Deserialize)]
struct RevokeSignals {
	#[serde(default, rename = "revokeId")]
	revoke_id: String,
}

#[derive(Serialize)]
struct AccountPatch {
	#[serde(rename = "accountError")]
	account_error: String,
	#[serde(rename = "leftAccount")]
	left_account: bool,
}

#[derive(Serialize)]
struct ErrorPatch {
	#[serde(rename = "accountError")]
	account_error: String,
}

fn clear_error() -> Result<PatchSignals> {
	PatchSignals::json(&ErrorPatch {
		account_error: String::new(),
	})
}

fn err_signal(error: &str) -> Result<PatchSignals> {
	PatchSignals::json(&ErrorPatch {
		account_error: error.to_owned(),
	})
}
