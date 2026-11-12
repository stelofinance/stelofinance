//! Transfer send: `account_search` procedure + `create_transfer`.

use crate::domain::{SearchScope, parse_recipient_term};
use crate::module_bindings::{
	AccountKind, AccountSearchHit, AccountSearchScope, MyAccountsTableAccess, account_lookup,
	account_search, create_transfer, finalize_transfer, my_accountsQueryTableAccess,
};
use spacetimedb_sdk::{DbContext, Table};

use super::call::{StdbError, call};
use super::connector::StdbConn;
use super::query::subscribe_once;

/// One recipient hit after procedure search + own-label attach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryHit {
	pub account_id: u64,
	pub address: String,
	pub primary_username: Option<String>,
	/// Caller's nickname when this is one of their other accounts.
	pub own_label: Option<String>,
	pub kind: AccountKind,
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
) -> Result<Vec<DirectoryHit>, StdbError> {
	let Some((scope, needle)) = parse_recipient_term(term) else {
		return Ok(Vec::new());
	};
	let hits = call_account_search(conn, needle, ledger_id, scope_arg(scope)).await?;
	let mine = fetch_own_labels(conn).await?;
	Ok(attach_own_labels(hits, &mine, exclude_id))
}

/// Public directory row for one account (invoice chrome).
pub async fn lookup_account(
	conn: &StdbConn,
	account_id: u64,
) -> Result<AccountSearchHit, StdbError> {
	let (reply, pending) = call("account_lookup");
	conn.db()
		.procedures
		.account_lookup_then(account_id, move |_, r| reply.send(r));
	pending.wait().await
}

async fn call_account_search(
	conn: &StdbConn,
	term: String,
	ledger_id: u64,
	scope: Option<AccountSearchScope>,
) -> Result<Vec<AccountSearchHit>, StdbError> {
	let (reply, pending) = call("account_search");
	conn.db()
		.procedures
		.account_search_then(term, ledger_id, scope, move |_, r| reply.send(r));
	pending.wait().await
}

async fn fetch_own_labels(conn: &StdbConn) -> Result<Vec<(u64, Option<String>)>, StdbError> {
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
			kind: h.kind,
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

pub async fn create_user_transfer(
	conn: &StdbConn,
	sending_account_id: u64,
	receiving_account_id: u64,
	amount: u64,
	memo: Option<String>,
	idempotency_key: String,
	pending: bool,
) -> Result<(), StdbError> {
	let (reply, response) = call("create_transfer");
	let sent = conn.db().reducers.create_transfer_then(
		sending_account_id,
		receiving_account_id,
		amount,
		memo,
		idempotency_key,
		pending,
		move |_, r| reply.send(r),
	);
	response.after_send(sent).await
}

pub async fn finalize_user_transfer(
	conn: &StdbConn,
	transfer_id: u64,
	amount: u64,
) -> Result<(), StdbError> {
	let (reply, pending) = call("finalize_transfer");
	let sent = conn
		.db()
		.reducers
		.finalize_transfer_then(transfer_id, amount, move |_, r| reply.send(r));
	pending.after_send(sent).await
}

#[cfg(test)]
mod tests {
	use super::*;

	fn hit(id: u64, address: &str, user: Option<&str>) -> AccountSearchHit {
		AccountSearchHit {
			account_id: id,
			address: address.to_owned(),
			ledger_id: 1,
			kind: AccountKind::Debit,
			primary_username: user.map(str::to_owned),
		}
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
