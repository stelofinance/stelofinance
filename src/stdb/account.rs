//! Single-account home: fetch, live subscribe, reducers.

use std::sync::Arc;

use crate::domain::role_rank;
use crate::einro::PooledConn;
use crate::module_bindings::{
	MyAccountMemberRow, MyAccountRow, MyAccountTokenRow, MyAccountsMembersTableAccess,
	MyAccountsTableAccess, MyAccountsTokensTableAccess, Role,
	SubscriptionHandle as ModuleSubHandle, UserSearchHit, create_account_token,
	grant_account_member, my_accounts_membersQueryTableAccess, my_accounts_tokensQueryTableAccess,
	my_accountsQueryTableAccess, revoke_account_member, revoke_account_tokens, set_account_label,
	set_account_primary, set_account_webhook, user_search,
};
use spacetimedb_sdk::{DbContext, Identity, SubscriptionHandle, Table, TableWithPrimaryKey};

use super::call::{StdbError, call};
use super::connector::StdbConn;
use super::query::{is_live_row_change, subscribe_once};

/// One API token for the account home list (never includes the secret).
#[derive(Clone, Debug)]
pub struct AccountTokenView {
	pub id: u64,
	pub label: String,
	pub created_micros: i64,
	pub created_by: Identity,
	pub created_by_username: Option<String>,
}

/// SSR / live snapshot for one account the caller can access.
#[derive(Clone)]
pub struct AccountHomeData {
	pub account: Option<MyAccountRow>,
	pub members: Vec<MyAccountMemberRow>,
	pub tokens: Vec<AccountTokenView>,
	pub has_other_primary: bool,
}

/// One-shot `my_accounts` + `my_accounts_members`, filtered to `account_id`.
pub async fn fetch_account_home(
	conn: &StdbConn,
	account_id: u64,
) -> Result<AccountHomeData, StdbError> {
	subscribe_once(
		conn,
		|b| {
			b.add_query(|q| q.from.my_accounts())
				.add_query(|q| q.from.my_accounts_members())
				.add_query(|q| q.from.my_accounts_tokens())
				.subscribe()
		},
		move |ctx| {
			collect_home(
				ctx.db().my_accounts().iter(),
				ctx.db().my_accounts_members().iter(),
				ctx.db().my_accounts_tokens().iter(),
				account_id,
			)
		},
	)
	.await
}

fn collect_home(
	accounts: impl Iterator<Item = MyAccountRow>,
	members: impl Iterator<Item = MyAccountMemberRow>,
	tokens: impl Iterator<Item = MyAccountTokenRow>,
	account_id: u64,
) -> AccountHomeData {
	let accounts: Vec<MyAccountRow> = accounts.collect();
	let account = accounts
		.iter()
		.find(|a| a.account_id == account_id)
		.cloned();
	let has_other_primary = account.as_ref().is_some_and(|acc| {
		accounts
			.iter()
			.any(|a| a.account_id != account_id && a.ledger_id == acc.ledger_id && a.is_primary)
	});
	let mut members: Vec<MyAccountMemberRow> =
		members.filter(|m| m.account_id == account_id).collect();
	sort_members(&mut members);

	let mut tokens: Vec<AccountTokenView> = tokens
		.filter(|t| t.account_id == account_id)
		.map(|t| AccountTokenView {
			id: t.id,
			label: t.label,
			created_micros: t.created_at.to_micros_since_unix_epoch(),
			created_by: t.created_by,
			created_by_username: t.created_by_username,
		})
		.collect();
	tokens.sort_by(|a, b| {
		b.created_micros
			.cmp(&a.created_micros)
			.then(b.id.cmp(&a.id))
	});

	AccountHomeData {
		account,
		members,
		tokens,
		has_other_primary,
	}
}

fn sort_members(members: &mut [MyAccountMemberRow]) {
	members.sort_by(|a, b| {
		role_rank(b.role)
			.cmp(&role_rank(a.role))
			.then(a.name.cmp(&b.name))
			.then(a.id.cmp(&b.id))
	});
}

pub async fn set_primary(conn: &StdbConn, account_id: u64, primary: bool) -> Result<(), StdbError> {
	let (reply, pending) = call("set_account_primary");
	let sent = conn
		.db()
		.reducers
		.set_account_primary_then(account_id, primary, move |_, r| reply.send(r));
	pending.after_send(sent).await
}

pub async fn set_label(
	conn: &StdbConn,
	account_id: u64,
	label: Option<String>,
) -> Result<(), StdbError> {
	let (reply, pending) = call("set_account_label");
	let sent = conn
		.db()
		.reducers
		.set_account_label_then(account_id, label, move |_, r| reply.send(r));
	pending.after_send(sent).await
}

/// Set or clear the account webhook URL. `None` / blank clears. Admin+ in the module.
pub async fn set_webhook(
	conn: &StdbConn,
	account_id: u64,
	webhook: Option<String>,
) -> Result<(), StdbError> {
	let (reply, pending) = call("set_account_webhook");
	let sent = conn
		.db()
		.reducers
		.set_account_webhook_then(account_id, webhook, move |_, r| reply.send(r));
	pending.after_send(sent).await
}

pub async fn grant_member(
	conn: &StdbConn,
	account_id: u64,
	member_id: Identity,
	role: Role,
) -> Result<(), StdbError> {
	let (reply, pending) = call("grant_account_member");
	let sent =
		conn.db()
			.reducers
			.grant_account_member_then(account_id, member_id, role, move |_, r| reply.send(r));
	pending.after_send(sent).await
}

/// Case-insensitive username prefix search (`user_search` procedure), skipping `exclude`.
pub async fn search_users(
	conn: &StdbConn,
	term: &str,
	exclude: &[Identity],
) -> Result<Vec<UserSearchHit>, StdbError> {
	let term = term.trim().to_owned();
	if term.is_empty() {
		return Ok(Vec::new());
	}
	let (reply, pending) = call("user_search");
	conn.db()
		.procedures
		.user_search_then(term, exclude.to_vec(), move |_, r| reply.send(r));
	pending.wait().await
}

pub async fn revoke_member(
	conn: &StdbConn,
	account_id: u64,
	member_id: Identity,
) -> Result<(), StdbError> {
	let (reply, pending) = call("revoke_account_member");
	let sent = conn
		.db()
		.reducers
		.revoke_account_member_then(account_id, member_id, move |_, r| reply.send(r));
	pending.after_send(sent).await
}

const TOKEN_ENTROPY_LEN: usize = 32;

/// Mint an account token. Returns the secret once. Admin+ is enforced in the module.
pub async fn create_account_api_token(
	conn: &StdbConn,
	account_id: u64,
	label: String,
) -> Result<String, StdbError> {
	let mut entropy = vec![0u8; TOKEN_ENTROPY_LEN];
	getrandom::fill(&mut entropy).map_err(|e| StdbError::internal(format!("entropy: {e}")))?;
	let entropy = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &entropy);

	let (reply, pending) = call("create_account_token");
	conn.db()
		.procedures
		.create_account_token_then(account_id, label, entropy, move |_, r| reply.send(r));
	pending.wait().await
}

/// Revoke listed tokens on an account. Admin+ is enforced in the module.
pub async fn revoke_account_api_tokens(
	conn: &StdbConn,
	account_id: u64,
	token_ids: Vec<u64>,
) -> Result<(), StdbError> {
	let (reply, pending) = call("revoke_account_tokens");
	let sent = conn
		.db()
		.reducers
		.revoke_account_tokens_then(account_id, token_ids, move |_, r| reply.send(r));
	pending.after_send(sent).await
}

/// Live `my_accounts` + `my_accounts_members` + `my_accounts_tokens` for one account.
pub struct LiveAccountHome {
	pooled: PooledConn<StdbConn>,
	sub: Option<ModuleSubHandle>,
	acc_insert: Option<crate::module_bindings::MyAccountsInsertCallbackId>,
	acc_update: Option<crate::module_bindings::MyAccountsUpdateCallbackId>,
	acc_delete: Option<crate::module_bindings::MyAccountsDeleteCallbackId>,
	mem_insert: Option<crate::module_bindings::MyAccountsMembersInsertCallbackId>,
	mem_update: Option<crate::module_bindings::MyAccountsMembersUpdateCallbackId>,
	mem_delete: Option<crate::module_bindings::MyAccountsMembersDeleteCallbackId>,
	tok_insert: Option<crate::module_bindings::MyAccountsTokensInsertCallbackId>,
	tok_update: Option<crate::module_bindings::MyAccountsTokensUpdateCallbackId>,
	tok_delete: Option<crate::module_bindings::MyAccountsTokensDeleteCallbackId>,
}

impl super::LiveSubscription for LiveAccountHome {
	fn conn(&self) -> &StdbConn {
		self.pooled.get()
	}
}

impl LiveAccountHome {
	pub fn start(
		pooled: PooledConn<StdbConn>,
		tx: tokio::sync::watch::Sender<Option<AccountHomeData>>,
		account_id: u64,
	) -> Result<Self, StdbError> {
		let send_snapshot: Arc<dyn Fn(AccountHomeData) + Send + Sync> = {
			let tx = tx.clone();
			Arc::new(move |data| {
				let _ = tx.send(Some(data));
			})
		};

		macro_rules! snap {
			($ctx:expr) => {
				collect_home(
					$ctx.db().my_accounts().iter(),
					$ctx.db().my_accounts_members().iter(),
					$ctx.db().my_accounts_tokens().iter(),
					account_id,
				)
			};
		}

		let acc_insert = {
			let send = Arc::clone(&send_snapshot);
			pooled.get().db().db.my_accounts().on_insert(move |ctx, _| {
				if is_live_row_change(&ctx.event) {
					send(snap!(ctx));
				}
			})
		};
		let acc_update = {
			let send = Arc::clone(&send_snapshot);
			pooled
				.get()
				.db()
				.db
				.my_accounts()
				.on_update(move |ctx, _, _| {
					if is_live_row_change(&ctx.event) {
						send(snap!(ctx));
					}
				})
		};
		let acc_delete = {
			let send = Arc::clone(&send_snapshot);
			pooled.get().db().db.my_accounts().on_delete(move |ctx, _| {
				if is_live_row_change(&ctx.event) {
					send(snap!(ctx));
				}
			})
		};
		let mem_insert = {
			let send = Arc::clone(&send_snapshot);
			pooled
				.get()
				.db()
				.db
				.my_accounts_members()
				.on_insert(move |ctx, _| {
					if is_live_row_change(&ctx.event) {
						send(snap!(ctx));
					}
				})
		};
		let mem_update = {
			let send = Arc::clone(&send_snapshot);
			pooled
				.get()
				.db()
				.db
				.my_accounts_members()
				.on_update(move |ctx, _, _| {
					if is_live_row_change(&ctx.event) {
						send(snap!(ctx));
					}
				})
		};
		let mem_delete = {
			let send = Arc::clone(&send_snapshot);
			pooled
				.get()
				.db()
				.db
				.my_accounts_members()
				.on_delete(move |ctx, _| {
					if is_live_row_change(&ctx.event) {
						send(snap!(ctx));
					}
				})
		};
		let tok_insert = {
			let send = Arc::clone(&send_snapshot);
			pooled
				.get()
				.db()
				.db
				.my_accounts_tokens()
				.on_insert(move |ctx, _| {
					if is_live_row_change(&ctx.event) {
						send(snap!(ctx));
					}
				})
		};
		let tok_update = {
			let send = Arc::clone(&send_snapshot);
			pooled
				.get()
				.db()
				.db
				.my_accounts_tokens()
				.on_update(move |ctx, _, _| {
					if is_live_row_change(&ctx.event) {
						send(snap!(ctx));
					}
				})
		};
		let tok_delete = {
			let send = Arc::clone(&send_snapshot);
			pooled
				.get()
				.db()
				.db
				.my_accounts_tokens()
				.on_delete(move |ctx, _| {
					if is_live_row_change(&ctx.event) {
						send(snap!(ctx));
					}
				})
		};
		let handle = pooled
			.get()
			.db()
			.subscription_builder()
			.on_applied({
				let send = Arc::clone(&send_snapshot);
				move |ctx| {
					send(snap!(ctx));
				}
			})
			.on_error(move |_ctx, err| {
				eprintln!("account home live subscribe error: {err}");
			})
			.add_query(|q| q.from.my_accounts())
			.add_query(|q| q.from.my_accounts_members())
			.add_query(|q| q.from.my_accounts_tokens())
			.subscribe();

		Ok(Self {
			pooled,
			sub: Some(handle),
			acc_insert: Some(acc_insert),
			acc_update: Some(acc_update),
			acc_delete: Some(acc_delete),
			mem_insert: Some(mem_insert),
			mem_update: Some(mem_update),
			mem_delete: Some(mem_delete),
			tok_insert: Some(tok_insert),
			tok_update: Some(tok_update),
			tok_delete: Some(tok_delete),
		})
	}
}

impl Drop for LiveAccountHome {
	fn drop(&mut self) {
		let tables = &self.pooled.get().db().db;
		if let Some(id) = self.acc_insert.take() {
			tables.my_accounts().remove_on_insert(id);
		}
		if let Some(id) = self.acc_update.take() {
			tables.my_accounts().remove_on_update(id);
		}
		if let Some(id) = self.acc_delete.take() {
			tables.my_accounts().remove_on_delete(id);
		}
		if let Some(id) = self.mem_insert.take() {
			tables.my_accounts_members().remove_on_insert(id);
		}
		if let Some(id) = self.mem_update.take() {
			tables.my_accounts_members().remove_on_update(id);
		}
		if let Some(id) = self.mem_delete.take() {
			tables.my_accounts_members().remove_on_delete(id);
		}
		if let Some(id) = self.tok_insert.take() {
			tables.my_accounts_tokens().remove_on_insert(id);
		}
		if let Some(id) = self.tok_update.take() {
			tables.my_accounts_tokens().remove_on_update(id);
		}
		if let Some(id) = self.tok_delete.take() {
			tables.my_accounts_tokens().remove_on_delete(id);
		}
		if let Some(sub) = self.sub.take() {
			let _ = sub.unsubscribe();
		}
	}
}
