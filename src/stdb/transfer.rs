//! Transfer send: `account_search` procedure + `create_transfer`.

use std::time::Duration;

use crate::module_bindings::{
	AccountKind, AccountSearchHit, AccountSearchScope, MyAccountRow, MyAccountsTableAccess, Role,
	account_search, create_transfer, my_accountsQueryTableAccess,
};
use spacetimedb_sdk::{DbContext, Table};
use tokio::task::spawn_blocking;

use super::account::role_rank;
use super::connector::StdbConn;
use super::query::subscribe_once;

const REDUCER_TIMEOUT: Duration = Duration::from_secs(15);

/// One recipient hit after procedure search + own-label attach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryHit {
	pub account_id: u64,
	pub address: String,
	pub primary_username: Option<String>,
	/// Caller's nickname when this is one of their other accounts.
	pub own_label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchScope {
	Username,
	Address,
	Both,
}

/// Debit accounts the caller can send from (Write+).
pub fn sendable_accounts(accounts: &[MyAccountRow]) -> Vec<MyAccountRow> {
	accounts
		.iter()
		.filter(|a| {
			matches!(a.kind, AccountKind::Debit) && role_rank(a.role) >= role_rank(Role::Write)
		})
		.cloned()
		.collect()
}

/// Strip one leading `@` / `#` and decide which directory fields to match.
pub fn parse_recipient_term(raw: &str) -> Option<(SearchScope, String)> {
	let t = raw.trim();
	if t.is_empty() {
		return None;
	}
	if let Some(rest) = t.strip_prefix('@') {
		let term = rest.trim();
		if term.is_empty() {
			return None;
		}
		Some((SearchScope::Username, term.to_ascii_uppercase()))
	} else if let Some(rest) = t.strip_prefix('#') {
		let term = rest.trim();
		if term.is_empty() {
			return None;
		}
		Some((SearchScope::Address, term.to_ascii_uppercase()))
	} else {
		Some((SearchScope::Both, t.to_ascii_uppercase()))
	}
}

fn scope_arg(scope: SearchScope) -> Option<AccountSearchScope> {
	match scope {
		SearchScope::Username => Some(AccountSearchScope::Username),
		SearchScope::Address => Some(AccountSearchScope::Address),
		SearchScope::Both => None,
	}
}

/// Procedure prefix search + `my_accounts` (for own labels).
pub async fn search_directory(
	conn: &StdbConn,
	ledger_id: u64,
	exclude_id: u64,
	term: &str,
) -> Result<Vec<DirectoryHit>, String> {
	let Some((scope, needle)) = parse_recipient_term(term) else {
		return Ok(Vec::new());
	};
	let hits = call_account_search(conn, needle, ledger_id, scope_arg(scope)).await?;
	let mine = fetch_own_labels(conn).await?;
	Ok(attach_own_labels(hits, &mine, exclude_id))
}

async fn call_account_search(
	conn: &StdbConn,
	term: String,
	ledger_id: u64,
	scope: Option<AccountSearchScope>,
) -> Result<Vec<AccountSearchHit>, String> {
	let (tx, rx) = std::sync::mpsc::sync_channel(1);
	conn.db()
		.procedures
		.account_search_then(term, ledger_id, scope, move |_ctx, result| {
			let outcome = match result {
				Ok(Ok(hits)) => Ok(hits),
				Ok(Err(e)) => Err(e),
				Err(e) => Err(e.to_string()),
			};
			let _ = tx.send(outcome);
		});
	let wait = spawn_blocking(move || {
		rx.recv_timeout(REDUCER_TIMEOUT)
			.map_err(|_| "account_search timed out".to_owned())
	})
	.await
	.map_err(|e| format!("account_search wait task: {e}"))?;
	wait?
}

async fn fetch_own_labels(conn: &StdbConn) -> Result<Vec<(u64, Option<String>)>, String> {
	subscribe_once(
		conn,
		|b| b.add_query(|q| q.from.my_accounts()).subscribe(),
		|ctx| {
			ctx.db()
				.my_accounts()
				.iter()
				.map(|a| (a.account_id, a.label.clone()))
				.collect()
		},
	)
	.await
}

fn attach_own_labels(
	hits: Vec<AccountSearchHit>,
	mine: &[(u64, Option<String>)],
	exclude_id: u64,
) -> Vec<DirectoryHit> {
	hits.into_iter()
		.filter(|h| h.account_id != exclude_id)
		.map(|h| DirectoryHit {
			account_id: h.account_id,
			address: h.address,
			primary_username: h.primary_username,
			own_label: mine
				.iter()
				.find(|(id, _)| *id == h.account_id)
				.and_then(|(_, l)| {
					l.as_deref()
						.map(str::trim)
						.filter(|s| !s.is_empty())
						.map(str::to_owned)
				}),
		})
		.collect()
}

pub fn new_idempotency_key() -> String {
	use std::time::{SystemTime, UNIX_EPOCH};
	let n = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_nanos())
		.unwrap_or(0);
	format!("{n:032x}")
}

pub fn map_transfer_error(err: &str) -> String {
	match err {
		"invalid balance" => "Not enough available balance.".to_owned(),
		"sender is receiver" => "Pick a different recipient.".to_owned(),
		"incompatible account ledgers" => "That account is a different asset.".to_owned(),
		"invalid quantity" => "Enter an amount greater than zero.".to_owned(),
		"idempotency key conflict" => {
			"This send was already submitted with different details. Refresh and try again."
				.to_owned()
		}
		other => other.to_owned(),
	}
}

/// Posted (not pending) `create_transfer`.
pub async fn create_user_transfer(
	conn: &StdbConn,
	sending_account_id: u64,
	receiving_account_id: u64,
	amount: u64,
	memo: Option<String>,
	idempotency_key: String,
) -> Result<(), String> {
	let (tx, rx) = std::sync::mpsc::sync_channel(1);
	conn.db()
		.reducers
		.create_transfer_then(
			sending_account_id,
			receiving_account_id,
			amount,
			memo,
			idempotency_key,
			false,
			move |_ctx, result| {
				let outcome = match result {
					Ok(Ok(())) => Ok(()),
					Ok(Err(e)) => Err(e),
					Err(e) => Err(e.to_string()),
				};
				let _ = tx.send(outcome);
			},
		)
		.map_err(|e| format!("create_transfer send: {e}"))?;

	let wait = spawn_blocking(move || {
		rx.recv_timeout(REDUCER_TIMEOUT)
			.map_err(|_| "create_transfer timed out".to_owned())
	})
	.await
	.map_err(|e| format!("create_transfer wait task: {e}"))?;
	wait?
}

#[cfg(test)]
mod tests {
	use super::*;

	fn hit(id: u64, address: &str, user: Option<&str>) -> AccountSearchHit {
		AccountSearchHit {
			account_id: id,
			address: address.to_owned(),
			ledger_id: 1,
			primary_username: user.map(str::to_owned),
		}
	}

	#[test]
	fn prefix_scope() {
		assert_eq!(
			parse_recipient_term("@Nin"),
			Some((SearchScope::Username, "NIN".into()))
		);
		assert_eq!(
			parse_recipient_term("#hex"),
			Some((SearchScope::Address, "HEX".into()))
		);
		assert_eq!(
			parse_recipient_term("  nin  "),
			Some((SearchScope::Both, "NIN".into()))
		);
		assert!(parse_recipient_term("@").is_none());
		assert!(parse_recipient_term("").is_none());
	}

	#[test]
	fn own_label_attached_and_sender_excluded() {
		let mine = [(9, Some("Guild chest".into())), (1, Some("Me".into()))];
		let hits = attach_own_labels(
			vec![hit(9, "HEXGUILD", None), hit(1, "MINE", None)],
			&mine,
			1,
		);
		assert_eq!(hits.len(), 1);
		assert_eq!(hits[0].account_id, 9);
		assert_eq!(hits[0].own_label.as_deref(), Some("Guild chest"));
	}
}
