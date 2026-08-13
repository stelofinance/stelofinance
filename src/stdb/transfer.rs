//! Transfer send: directory search + `create_transfer`.

use std::time::Duration;

use crate::module_bindings::{
	AccountDirectoryRow, AccountDirectoryTableAccess, AccountKind, MyAccountRow,
	MyAccountsTableAccess, Role, account_directoryQueryTableAccess, create_transfer,
	my_accountsQueryTableAccess,
};
use spacetimedb_sdk::{DbContext, Table};
use tokio::task::spawn_blocking;

use super::account::role_rank;
use super::connector::StdbConn;
use super::query::subscribe_once;

const REDUCER_TIMEOUT: Duration = Duration::from_secs(15);
const SEARCH_LIMIT: usize = 10;

/// One recipient hit after ledger filter + ranking.
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
		Some((SearchScope::Username, term.to_ascii_lowercase()))
	} else if let Some(rest) = t.strip_prefix('#') {
		let term = rest.trim();
		if term.is_empty() {
			return None;
		}
		Some((SearchScope::Address, term.to_ascii_lowercase()))
	} else {
		Some((SearchScope::Both, t.to_ascii_lowercase()))
	}
}

/// One-shot `account_directory` + `my_accounts` (for own labels). Filter/rank on the edge.
pub async fn search_directory(
	conn: &StdbConn,
	ledger_id: u64,
	exclude_id: u64,
	term: &str,
) -> Result<Vec<DirectoryHit>, String> {
	let Some((scope, needle)) = parse_recipient_term(term) else {
		return Ok(Vec::new());
	};
	subscribe_once(
		conn,
		|b| {
			b.add_query(|q| q.from.account_directory())
				.add_query(|q| q.from.my_accounts())
				.subscribe()
		},
		move |ctx| {
			let mine: Vec<(u64, Option<String>)> = ctx
				.db()
				.my_accounts()
				.iter()
				.map(|a| (a.account_id, a.label.clone()))
				.collect();
			rank_hits(
				ctx.db().account_directory().iter(),
				&mine,
				ledger_id,
				exclude_id,
				scope,
				&needle,
				SEARCH_LIMIT,
			)
		},
	)
	.await
}

pub fn rank_hits(
	rows: impl Iterator<Item = AccountDirectoryRow>,
	mine: &[(u64, Option<String>)],
	ledger_id: u64,
	exclude_id: u64,
	scope: SearchScope,
	needle: &str,
	limit: usize,
) -> Vec<DirectoryHit> {
	let mut scored: Vec<(u8, u8, String, DirectoryHit)> = Vec::new();
	for row in rows {
		if row.ledger_id != ledger_id || row.account_id == exclude_id {
			continue;
		}
		let Some((rank, field)) = best_rank(&row, scope, needle) else {
			continue;
		};
		let own_label = mine
			.iter()
			.find(|(id, _)| *id == row.account_id)
			.and_then(|(_, l)| {
				l.as_deref()
					.map(str::trim)
					.filter(|s| !s.is_empty())
					.map(str::to_owned)
			});
		let sort_name = row
			.primary_username
			.as_deref()
			.map(str::to_ascii_lowercase)
			.unwrap_or_else(|| row.address.to_ascii_lowercase());
		scored.push((
			rank,
			field,
			sort_name,
			DirectoryHit {
				account_id: row.account_id,
				address: row.address,
				primary_username: row.primary_username,
				own_label,
			},
		));
	}
	scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
	scored.into_iter().take(limit).map(|s| s.3).collect()
}

/// Lower is better. Field 0 = username, 1 = address (username wins ties).
fn best_rank(row: &AccountDirectoryRow, scope: SearchScope, needle: &str) -> Option<(u8, u8)> {
	let user = row
		.primary_username
		.as_deref()
		.filter(|s| !s.is_empty())
		.and_then(|u| field_rank(u, needle).map(|r| (r, 0u8)));
	let addr = field_rank(&row.address, needle).map(|r| (r, 1u8));
	match scope {
		SearchScope::Username => user,
		SearchScope::Address => addr,
		SearchScope::Both => match (user, addr) {
			(Some(u), Some(a)) => Some(if u.0 <= a.0 { u } else { a }),
			(Some(u), None) => Some(u),
			(None, Some(a)) => Some(a),
			(None, None) => None,
		},
	}
}

fn field_rank(hay: &str, needle: &str) -> Option<u8> {
	let h = hay.to_ascii_lowercase();
	if h == needle {
		Some(0)
	} else if h.starts_with(needle) {
		Some(1)
	} else if h.contains(needle) {
		Some(2)
	} else {
		None
	}
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

	fn row(id: u64, address: &str, ledger: u64, user: Option<&str>) -> AccountDirectoryRow {
		AccountDirectoryRow {
			account_id: id,
			address: address.to_owned(),
			ledger_id: ledger,
			primary_username: user.map(str::to_owned),
		}
	}

	#[test]
	fn prefix_scope() {
		assert_eq!(
			parse_recipient_term("@Nin"),
			Some((SearchScope::Username, "nin".into()))
		);
		assert_eq!(
			parse_recipient_term("#hex"),
			Some((SearchScope::Address, "hex".into()))
		);
		assert_eq!(
			parse_recipient_term("  nin  "),
			Some((SearchScope::Both, "nin".into()))
		);
		assert!(parse_recipient_term("@").is_none());
		assert!(parse_recipient_term("").is_none());
	}

	#[test]
	fn ranks_exact_then_prefix_then_contains() {
		let rows = [
			row(1, "XXNINXX", 1, Some("xninx")),
			row(2, "OTHER", 1, Some("nintron")),
			row(3, "NINWALLET", 1, None),
			row(4, "ZZZ", 1, Some("bob")),
			row(5, "SKIP", 2, Some("nintron")),
			row(6, "MINE", 1, Some("me")),
		];
		let mine = [(6, Some("Guild chest".into()))];
		let hits = rank_hits(rows.into_iter(), &mine, 1, 6, SearchScope::Both, "nin", 10);
		let ids: Vec<u64> = hits.iter().map(|h| h.account_id).collect();
		// nintron = username prefix (1,0); NINWALLET = address prefix (1,1);
		// xninx = username contains (2,0). Exclude 6; drop other ledger.
		assert_eq!(ids, vec![2, 3, 1]);
	}

	#[test]
	fn at_prefix_skips_address_only() {
		let rows = [
			row(1, "NINWALLET", 1, None),
			row(2, "ZZ", 1, Some("nintron")),
		];
		let hits = rank_hits(
			rows.into_iter(),
			&[],
			1,
			0,
			SearchScope::Username,
			"nin",
			10,
		);
		assert_eq!(hits.len(), 1);
		assert_eq!(hits[0].account_id, 2);
	}

	#[test]
	fn own_label_attached() {
		let rows = [row(9, "HEXGUILD", 1, None)];
		let mine = [(9, Some("Guild chest".into()))];
		let hits = rank_hits(
			rows.into_iter(),
			&mine,
			1,
			1,
			SearchScope::Address,
			"hex",
			10,
		);
		assert_eq!(hits[0].own_label.as_deref(), Some("Guild chest"));
	}
}
