//! Single-account home: fetch, live subscribe, reducers.

use std::sync::Arc;
use std::time::Duration;

use crate::einro::PooledConn;
use crate::module_bindings::{
	MyAccountMemberRow, MyAccountRow, MyAccountsMembersTableAccess, MyAccountsTableAccess, Reducer,
	Role, SubscriptionHandle as ModuleSubHandle, User, UserTableAccess, grant_account_member,
	my_accounts_membersQueryTableAccess, my_accountsQueryTableAccess, revoke_account_member,
	set_account_label, set_account_primary, userQueryTableAccess,
};
use spacetimedb_sdk::{DbContext, Event, Identity, SubscriptionHandle, Table, TableWithPrimaryKey};
use tokio::task::spawn_blocking;

use super::connector::StdbConn;
use super::query::subscribe_once;

const REDUCER_TIMEOUT: Duration = Duration::from_secs(15);

/// SSR / live snapshot for one account the caller can access.
#[derive(Clone)]
pub struct AccountHomeData {
	pub account: Option<MyAccountRow>,
	pub members: Vec<MyAccountMemberRow>,
	pub has_other_primary: bool,
}

/// One-shot `my_accounts` + `my_accounts_members`, filtered to `account_id`.
pub async fn fetch_account_home(
	conn: &StdbConn,
	account_id: u64,
) -> Result<AccountHomeData, String> {
	subscribe_once(
		conn,
		|b| {
			b.add_query(|q| q.from.my_accounts())
				.add_query(|q| q.from.my_accounts_members())
				.subscribe()
		},
		move |ctx| {
			collect_home(
				ctx.db().my_accounts().iter(),
				ctx.db().my_accounts_members().iter(),
				account_id,
			)
		},
	)
	.await
}

fn collect_home(
	accounts: impl Iterator<Item = MyAccountRow>,
	members: impl Iterator<Item = MyAccountMemberRow>,
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
	AccountHomeData {
		account,
		members,
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

pub fn role_rank(role: Role) -> u8 {
	match role {
		Role::Read => 0,
		Role::Write => 1,
		Role::Admin => 2,
		Role::Owner => 3,
	}
}

pub fn parse_role(s: &str) -> Option<Role> {
	match s.trim().to_ascii_lowercase().as_str() {
		"read" => Some(Role::Read),
		"write" => Some(Role::Write),
		"admin" => Some(Role::Admin),
		"owner" => Some(Role::Owner),
		_ => None,
	}
}

fn map_reducer(result: Result<Result<(), String>, impl ToString>) -> Result<(), String> {
	match result {
		Ok(Ok(())) => Ok(()),
		Ok(Err(e)) => Err(e),
		Err(e) => Err(e.to_string()),
	}
}

async fn wait_reducer(
	start: impl FnOnce(std::sync::mpsc::SyncSender<Result<(), String>>) -> Result<(), String>,
) -> Result<(), String> {
	let (tx, rx) = std::sync::mpsc::sync_channel(1);
	start(tx)?;
	let wait = spawn_blocking(move || {
		rx.recv_timeout(REDUCER_TIMEOUT)
			.map_err(|_| "reducer timed out".to_owned())
	})
	.await
	.map_err(|e| format!("reducer wait task: {e}"))?;
	wait?
}

pub async fn set_primary(conn: &StdbConn, account_id: u64, primary: bool) -> Result<(), String> {
	wait_reducer(|tx| {
		conn.db()
			.reducers
			.set_account_primary_then(account_id, primary, move |_, r| {
				let _ = tx.send(map_reducer(r));
			})
			.map_err(|e| format!("set_account_primary send: {e}"))
	})
	.await
}

pub async fn set_label(
	conn: &StdbConn,
	account_id: u64,
	label: Option<String>,
) -> Result<(), String> {
	wait_reducer(|tx| {
		conn.db()
			.reducers
			.set_account_label_then(account_id, label, move |_, r| {
				let _ = tx.send(map_reducer(r));
			})
			.map_err(|e| format!("set_account_label send: {e}"))
	})
	.await
}

pub async fn grant_member(
	conn: &StdbConn,
	account_id: u64,
	member_id: Identity,
	role: Role,
) -> Result<(), String> {
	wait_reducer(|tx| {
		conn.db()
			.reducers
			.grant_account_member_then(account_id, member_id, role, move |_, r| {
				let _ = tx.send(map_reducer(r));
			})
			.map_err(|e| format!("grant_account_member send: {e}"))
	})
	.await
}

const USER_SEARCH_LIMIT: usize = 10;

/// Exclusive end of a prefix range: `"JA"` → `Some("JB")`.
/// `None` means no upper bound (prefix is empty after carry, or last scalar is `char::MAX`).
fn exclusive_prefix_end(prefix: &str) -> Option<String> {
	let mut chars: Vec<char> = prefix.chars().collect();
	while let Some(c) = chars.pop() {
		let next_cp = (c as u32).saturating_add(1);
		if let Some(next) = char::from_u32(next_cp) {
			chars.push(next);
			return Some(chars.into_iter().collect());
		}
		// Skip the UTF-16 surrogate gap so the bound stays a valid exclusive end.
		if next_cp == 0xD800 {
			chars.push('\u{E000}');
			return Some(chars.into_iter().collect());
		}
	}
	None
}

/// Case-insensitive username prefix search on the public `user` table.
pub async fn search_users(
	conn: &StdbConn,
	term: &str,
	exclude: &[Identity],
) -> Result<Vec<User>, String> {
	let start = term.trim().to_ascii_uppercase();
	if start.is_empty() {
		return Ok(Vec::new());
	}
	let end = exclusive_prefix_end(&start);
	let exclude: Vec<Identity> = exclude.to_vec();
	let start_for_collect = start.clone();
	subscribe_once(
		conn,
		|b| match end {
			Some(end) => {
				let start = start.clone();
				b.add_query(move |q| {
					let start = start.clone();
					let end = end.clone();
					q.from.user().r#where(move |u| {
						u.bitcraft_username_normalized
							.gte(start.clone())
							.and(u.bitcraft_username_normalized.lt(end.clone()))
					})
				})
				.subscribe()
			}
			None => {
				let start = start.clone();
				b.add_query(move |q| {
					let start = start.clone();
					q.from
						.user()
						.r#where(move |u| u.bitcraft_username_normalized.gte(start.clone()))
				})
				.subscribe()
			}
		},
		move |ctx| {
			let mut rows: Vec<User> = ctx
				.db()
				.user()
				.iter()
				.filter(|u| {
					!exclude.contains(&u.id)
						&& u.bitcraft_username_normalized
							.starts_with(&start_for_collect)
				})
				.collect();
			rows.sort_by(|a, b| a.bitcraft_username.cmp(&b.bitcraft_username));
			rows.truncate(USER_SEARCH_LIMIT);
			rows
		},
	)
	.await
}

pub async fn revoke_member(
	conn: &StdbConn,
	account_id: u64,
	member_id: Identity,
) -> Result<(), String> {
	wait_reducer(|tx| {
		conn.db()
			.reducers
			.revoke_account_member_then(account_id, member_id, move |_, r| {
				let _ = tx.send(map_reducer(r));
			})
			.map_err(|e| format!("revoke_account_member send: {e}"))
	})
	.await
}

fn is_live_row_change(event: &Event<Reducer>) -> bool {
	matches!(event, Event::Reducer(_) | Event::Transaction)
}

/// Live `my_accounts` + `my_accounts_members` for one account.
pub struct LiveAccountHome {
	pooled: PooledConn<StdbConn>,
	sub: Option<ModuleSubHandle>,
	acc_insert: Option<crate::module_bindings::MyAccountsInsertCallbackId>,
	acc_update: Option<crate::module_bindings::MyAccountsUpdateCallbackId>,
	acc_delete: Option<crate::module_bindings::MyAccountsDeleteCallbackId>,
	mem_insert: Option<crate::module_bindings::MyAccountsMembersInsertCallbackId>,
	mem_update: Option<crate::module_bindings::MyAccountsMembersUpdateCallbackId>,
	mem_delete: Option<crate::module_bindings::MyAccountsMembersDeleteCallbackId>,
}

impl LiveAccountHome {
	pub fn start(
		pooled: PooledConn<StdbConn>,
		tx: tokio::sync::watch::Sender<Option<AccountHomeData>>,
		account_id: u64,
		snapshot_on_applied: bool,
	) -> Result<Self, String> {
		let send_snapshot: Arc<dyn Fn(Vec<MyAccountRow>, Vec<MyAccountMemberRow>) + Send + Sync> = {
			let tx = tx.clone();
			Arc::new(move |accounts, members| {
				let _ = tx.send(Some(collect_home(
					accounts.into_iter(),
					members.into_iter(),
					account_id,
				)));
			})
		};

		let acc_insert = {
			let send = Arc::clone(&send_snapshot);
			pooled.get().db().db.my_accounts().on_insert(move |ctx, _| {
				if is_live_row_change(&ctx.event) {
					send(
						ctx.db().my_accounts().iter().collect(),
						ctx.db().my_accounts_members().iter().collect(),
					);
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
						send(
							ctx.db().my_accounts().iter().collect(),
							ctx.db().my_accounts_members().iter().collect(),
						);
					}
				})
		};
		let acc_delete = {
			let send = Arc::clone(&send_snapshot);
			pooled.get().db().db.my_accounts().on_delete(move |ctx, _| {
				if is_live_row_change(&ctx.event) {
					send(
						ctx.db().my_accounts().iter().collect(),
						ctx.db().my_accounts_members().iter().collect(),
					);
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
						send(
							ctx.db().my_accounts().iter().collect(),
							ctx.db().my_accounts_members().iter().collect(),
						);
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
						send(
							ctx.db().my_accounts().iter().collect(),
							ctx.db().my_accounts_members().iter().collect(),
						);
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
						send(
							ctx.db().my_accounts().iter().collect(),
							ctx.db().my_accounts_members().iter().collect(),
						);
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
					if snapshot_on_applied {
						send(
							ctx.db().my_accounts().iter().collect(),
							ctx.db().my_accounts_members().iter().collect(),
						);
					}
				}
			})
			.on_error(move |_ctx, err| {
				eprintln!("account home live subscribe error: {err}");
			})
			.add_query(|q| q.from.my_accounts())
			.add_query(|q| q.from.my_accounts_members())
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
		if let Some(sub) = self.sub.take() {
			let _ = sub.unsubscribe();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::exclusive_prefix_end;

	#[test]
	fn exclusive_prefix_end_ascii() {
		assert_eq!(exclusive_prefix_end("JA").as_deref(), Some("JB"));
		assert_eq!(exclusive_prefix_end("JZ").as_deref(), Some("J["));
		assert_eq!(exclusive_prefix_end("Z").as_deref(), Some("["));
		assert_eq!(exclusive_prefix_end(""), None);
	}

	#[test]
	fn exclusive_prefix_end_max_char_carries() {
		let prefix = format!("A{}", char::MAX);
		assert_eq!(exclusive_prefix_end(&prefix).as_deref(), Some("B"));
		assert_eq!(exclusive_prefix_end(&char::MAX.to_string()), None);
	}
}
