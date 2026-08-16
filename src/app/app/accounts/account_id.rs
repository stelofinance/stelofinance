//! `GET /app/accounts/{account_id}` — H2 account home.

mod markup;
mod tokens;
mod updates;
mod users;

use crate::auth::require_user;
use crate::module_bindings::{AccountKind, Role};
use crate::stdb::account::{
	fetch_account_home, grant_member, parse_role, revoke_member, role_rank, set_label, set_primary,
};
use crate::stdb::{StdbError, acquire_user_db};
use markup::{HomeChrome, account_home};
use serde::{Deserialize, Serialize};
use spacetimedb_sdk::Identity;
use tokens::token_forms;
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchSignals, Signals},
	router::{
		error::{internal_server_error, not_found},
		page, path_param, route,
	},
	view::{component, view},
};

#[path_param(error = not_found)]
pub struct AccountId(u64);

#[page]
async fn show(cx: &Cx) -> Result {
	let user = require_user(cx).await?;
	let account_id = *path_param::<AccountId>(cx)?;
	let conn = acquire_user_db(cx).await?;
	let data = fetch_account_home(conn.get(), account_id)
		.await
		.map_err(|e| internal_server_error(StdbError(e)))?;
	let Some(acc) = &data.account else {
		return Err(not_found().into());
	};

	let admin_plus = role_rank(acc.role) >= role_rank(Role::Admin);
	let label_value = acc.label.clone().unwrap_or_default();
	let signals = format!(
		"{{copiedId:0,label:{},userSearch:'',memberId:'',memberName:'',addRole:'write',editMemberId:'',editRole:'',revokeId:'',accountError:'',leftAccount:false,creatingToken:false,tokenLabel:'',newToken:'',tokenError:'',tokenCopied:false,revokeTokenId:0,revokeTokenLabel:''}}",
		js_single(&label_value)
	);
	let updates = format!("@get('/app/accounts/{account_id}/updates')");
	let members_url = format!("/app/accounts/{account_id}/members");
	let users_url = format!("/app/accounts/{account_id}/users");
	let label_url = format!("/app/accounts/{account_id}/label");
	let chrome = HomeChrome {
		caller_id: user.identity,
		caller_username: user.bitcraft_username.clone(),
	};

	view! {
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
			account_home(data: data, chrome: chrome)
			if admin_plus {
				token_forms(account_id: account_id)
				admin_forms(account_id: account_id, members_url: members_url, users_url: users_url, label_url: label_url)
			}
			<p
				class="mt-4 text-sm text-red-400"
				data-show="$accountError"
				data-text="$accountError"
			></p>
		</main>
	}
}

#[component]
async fn admin_forms(
	account_id: u64,
	members_url: String,
	users_url: String,
	label_url: String,
) -> Result {
	let _ = account_id;
	let search = format!("@get('{users_url}')");
	view! {
		<section class="mt-8 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5">
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

		<section class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4 sm:p-5">
			<h2 class="text-sm font-medium uppercase tracking-wide text-neutral-400">
				"Add person"
			</h2>
			<p class="mt-1 text-sm text-neutral-400">
				"Search a BitCraft username, then pick them. Read sees, Write can send, Admin manages people."
			</p>
			<input type="hidden" data-bind="memberId">
			<div data-show="!$memberId">
				<input
					type="text"
					class="mt-3 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm placeholder:text-neutral-400"
					data-bind="userSearch"
					placeholder="Search username…"
					autocomplete="off"
					data-on:input__debounce.300ms=(search)
				>
				<div id="user-search-results"></div>
			</div>
			<div
				class="mt-3 flex items-center justify-between rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2"
				data-show="$memberId"
				style="display: none"
			>
				<p class="text-sm">
					"@"
					<span data-text="$memberName"></span>
				</p>
				<button
					type="button"
					class="cursor-pointer text-sm text-neutral-300 hover:text-white"
					data-on:click=(format!("$memberId = ''; $memberName = ''; $userSearch = ''; @get('{users_url}')"))
				>
					"Clear"
				</button>
			</div>
			<div class="mt-3 flex flex-col gap-2 sm:flex-row">
				<select
					class="cursor-pointer rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm"
					data-bind="addRole"
				>
					<option value="read">"Read"</option>
					<option value="write">"Write"</option>
					<option value="admin">"Admin"</option>
				</select>
				<button
					type="button"
					class="cursor-pointer rounded-md bg-anakiwa-700 px-4 py-2 text-sm font-medium text-white hover:bg-anakiwa-600"
					data-on:click=(format!("@post('{members_url}')"))
					data-indicator="addingMember"
					data-attr-disabled="$addingMember || !$memberId"
				>
					"Add"
				</button>
			</div>
		</section>
	}
}

#[route(POST "/app/accounts/{account_id}/primary")]
async fn set_primary_route(cx: &Cx) -> Result<PatchSignals> {
	let account_id = *path_param::<AccountId>(cx)?;
	let conn = acquire_user_db(cx).await?;
	let data = match fetch_account_home(conn.get(), account_id).await {
		Ok(d) => d,
		Err(e) => return err_signal(&e),
	};
	let Some(acc) = data.account else {
		return err_signal("Account not found.");
	};
	if !matches!(acc.kind, AccountKind::Debit) {
		return err_signal("Only debit accounts can be primary.");
	}
	match set_primary(conn.get(), account_id, !acc.is_primary).await {
		Ok(()) => clear_error(),
		Err(e) => err_signal(&e),
	}
}

#[route(POST "/app/accounts/{account_id}/label")]
async fn save_label(cx: &Cx, Signals(form): Signals<LabelSignals>) -> Result<PatchSignals> {
	let account_id = *path_param::<AccountId>(cx)?;
	let conn = acquire_user_db(cx).await?;
	let trimmed = form.label.trim();
	let label = if trimmed.is_empty() {
		None
	} else {
		Some(trimmed.to_owned())
	};
	match set_label(conn.get(), account_id, label).await {
		Ok(()) => clear_error(),
		Err(e) => err_signal(&e),
	}
}

#[route(POST "/app/accounts/{account_id}/members")]
async fn add_member(cx: &Cx, Signals(form): Signals<MemberSignals>) -> Result<PatchSignals> {
	let account_id = *path_param::<AccountId>(cx)?;
	let editing = !form.edit_member_id.trim().is_empty();
	let id_hex = if editing {
		form.edit_member_id.trim()
	} else {
		form.member_id.trim()
	};
	if id_hex.is_empty() {
		return err_signal("Pick a person from the search results.");
	}
	let Ok(member_id) = Identity::from_hex(id_hex) else {
		return err_signal("Invalid user.");
	};
	let role_str = if editing {
		form.edit_role.as_str()
	} else {
		form.add_role.as_str()
	};
	let Some(role) = parse_role(role_str) else {
		return err_signal("Choose a role.");
	};
	let conn = acquire_user_db(cx).await?;
	match grant_member(conn.get(), account_id, member_id, role).await {
		Ok(()) => {
			if editing {
				clear_error()
			} else {
				PatchSignals::json(&AccountPatch {
					account_error: String::new(),
					member_id: String::new(),
					member_name: String::new(),
					user_search: String::new(),
					add_role: "write".into(),
					edit_member_id: String::new(),
					left_account: false,
				})
			}
		}
		Err(e) => err_signal(&e),
	}
}

#[route(POST "/app/accounts/{account_id}/revoke")]
async fn revoke_member_route(
	cx: &Cx,
	Signals(form): Signals<RevokeSignals>,
) -> Result<PatchSignals> {
	let account_id = *path_param::<AccountId>(cx)?;
	let Ok(member_id) = Identity::from_hex(form.revoke_id.trim()) else {
		return err_signal("Invalid member.");
	};
	let conn = acquire_user_db(cx).await?;
	match revoke_member(conn.get(), account_id, member_id).await {
		Ok(()) => clear_error(),
		Err(e) => err_signal(&e),
	}
}

#[route(POST "/app/accounts/{account_id}/leave")]
async fn leave_account(cx: &Cx) -> Result<PatchSignals> {
	let user = require_user(cx).await?;
	let account_id = *path_param::<AccountId>(cx)?;
	let conn = acquire_user_db(cx).await?;
	match revoke_member(conn.get(), account_id, user.identity).await {
		Ok(()) => PatchSignals::json(&AccountPatch {
			account_error: String::new(),
			member_id: String::new(),
			member_name: String::new(),
			user_search: String::new(),
			add_role: "write".into(),
			edit_member_id: String::new(),
			left_account: true,
		}),
		Err(e) => err_signal(&e),
	}
}

#[derive(Debug, Deserialize)]
struct LabelSignals {
	#[serde(default)]
	label: String,
}

#[derive(Debug, Deserialize)]
struct MemberSignals {
	#[serde(default, rename = "memberId")]
	member_id: String,
	#[serde(default, rename = "addRole")]
	add_role: String,
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
	#[serde(rename = "memberId")]
	member_id: String,
	#[serde(rename = "memberName")]
	member_name: String,
	#[serde(rename = "userSearch")]
	user_search: String,
	#[serde(rename = "addRole")]
	add_role: String,
	#[serde(rename = "editMemberId")]
	edit_member_id: String,
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

fn js_single(s: &str) -> String {
	let mut out = String::from("'");
	for c in s.chars() {
		match c {
			'\\' => out.push_str("\\\\"),
			'\'' => out.push_str("\\'"),
			'\n' => out.push_str("\\n"),
			c => out.push(c),
		}
	}
	out.push('\'');
	out
}
