//! Prefix recipient search. Scope is chosen by the caller (`@` / `#` / both).
// TODO: Clean this mud up

use crate::tables::*;
use spacetimedb::{Identity, ProcedureContext, SpacetimeType, TxContext, procedure};

const SEARCH_LIMIT: usize = 10;

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountSearchScope {
	Username,
	Address,
}

#[derive(SpacetimeType, Clone, Debug, PartialEq, Eq)]
pub struct AccountSearchHit {
	pub account_id: u64,
	pub address: String,
	pub ledger_id: u64,
	pub primary_username: Option<String>,
}

/// Prefix search on public address / primary username within one ledger.
///
/// `scope = None` matches both fields. `term` is treated as an ASCII-upper prefix.
/// At most 10 ranked hits. Empty term → empty list.
#[procedure]
pub fn account_search(
	ctx: &mut ProcedureContext,
	term: String,
	ledger_id: u64,
	scope: Option<AccountSearchScope>,
) -> Result<Vec<AccountSearchHit>, String> {
	ctx.try_with_tx(move |tx| account_search_tx(tx, term.clone(), ledger_id, scope))
}

fn account_search_tx(
	tx: &TxContext,
	term: String,
	ledger_id: u64,
	scope: Option<AccountSearchScope>,
) -> Result<Vec<AccountSearchHit>, String> {
	let needle = term.trim().to_ascii_uppercase();
	if needle.is_empty() {
		return Ok(Vec::new());
	}
	if tx.db.ledger().id().find(&ledger_id).is_none() {
		return Err("ledger not found".to_string());
	}

	let want_user = !matches!(scope, Some(AccountSearchScope::Address));
	let want_addr = !matches!(scope, Some(AccountSearchScope::Username));
	let end = exclusive_prefix_end(&needle);

	let mut hits: Vec<AccountSearchHit> = Vec::new();
	if want_addr {
		collect_address_hits(tx, ledger_id, &needle, end.as_deref(), &mut hits);
	}
	if want_user {
		collect_username_hits(tx, ledger_id, &needle, end.as_deref(), &mut hits);
	}

	Ok(rank_hits(hits, &needle, SEARCH_LIMIT))
}

fn collect_address_hits(
	tx: &TxContext,
	ledger_id: u64,
	start: &str,
	end: Option<&str>,
	out: &mut Vec<AccountSearchHit>,
) {
	let rows: Vec<Account> = if let Some(end) = end {
		tx.db
			.account()
			.by_ledger_and_address()
			.filter((ledger_id, start..end))
			.collect()
	} else {
		tx.db
			.account()
			.by_ledger_and_address()
			.filter((ledger_id, start..))
			.collect()
	};
	for acc in rows {
		push_unique(out, hit_from_account(tx, &acc));
	}
}

fn collect_username_hits(
	tx: &TxContext,
	ledger_id: u64,
	start: &str,
	end: Option<&str>,
	out: &mut Vec<AccountSearchHit>,
) {
	let users: Vec<User> = if let Some(end) = end {
		tx.db
			.user()
			.bitcraft_username_normalized()
			.filter(start..end)
			.collect()
	} else {
		tx.db
			.user()
			.bitcraft_username_normalized()
			.filter(start..)
			.collect()
	};
	for user in users {
		for acc in tx
			.db
			.account()
			.by_user_and_ledger()
			.filter((user.id, ledger_id))
		{
			push_unique(out, hit_from_account(tx, &acc));
		}
	}
}

fn hit_from_account(tx: &TxContext, acc: &Account) -> AccountSearchHit {
	AccountSearchHit {
		account_id: acc.id,
		address: acc.address.clone(),
		ledger_id: acc.ledger_id,
		primary_username: username_for(tx, acc.user_id),
	}
}

fn username_for(tx: &TxContext, user_id: Identity) -> Option<String> {
	if user_id == Identity::ZERO {
		return None;
	}
	tx.db
		.user()
		.id()
		.find(&user_id)
		.map(|u| u.bitcraft_username)
}

fn push_unique(out: &mut Vec<AccountSearchHit>, hit: AccountSearchHit) {
	if !out.iter().any(|h| h.account_id == hit.account_id) {
		out.push(hit);
	}
}

/// Exact, then prefix. Username beats address on a tie. Lower is better.
fn rank_hits(hits: Vec<AccountSearchHit>, needle: &str, limit: usize) -> Vec<AccountSearchHit> {
	let mut scored: Vec<(u8, u8, String, AccountSearchHit)> = Vec::new();
	for hit in hits {
		let Some((rank, field)) = best_rank(&hit, needle) else {
			continue;
		};
		let sort_name = hit
			.primary_username
			.as_deref()
			.map(str::to_ascii_uppercase)
			.unwrap_or_else(|| hit.address.to_ascii_uppercase());
		scored.push((rank, field, sort_name, hit));
	}
	scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
	scored.into_iter().take(limit).map(|s| s.3).collect()
}

fn best_rank(hit: &AccountSearchHit, needle: &str) -> Option<(u8, u8)> {
	let user = hit
		.primary_username
		.as_deref()
		.filter(|s| !s.is_empty())
		.and_then(|u| field_rank(u, needle).map(|r| (r, 0u8)));
	let addr = field_rank(&hit.address, needle).map(|r| (r, 1u8));
	match (user, addr) {
		(Some(u), Some(a)) => Some(if u.0 <= a.0 { u } else { a }),
		(Some(u), None) => Some(u),
		(None, Some(a)) => Some(a),
		(None, None) => None,
	}
}

fn field_rank(hay: &str, needle: &str) -> Option<u8> {
	let h = hay.to_ascii_uppercase();
	if h == needle {
		Some(0)
	} else if h.starts_with(needle) {
		Some(1)
	} else {
		None
	}
}

/// Exclusive end of a prefix range: `"JA"` → `Some("JB")`.
fn exclusive_prefix_end(prefix: &str) -> Option<String> {
	let mut chars: Vec<char> = prefix.chars().collect();
	while let Some(c) = chars.pop() {
		let next_cp = (c as u32).saturating_add(1);
		if let Some(next) = char::from_u32(next_cp) {
			chars.push(next);
			return Some(chars.into_iter().collect());
		}
		if next_cp == 0xD800 {
			chars.push('\u{E000}');
			return Some(chars.into_iter().collect());
		}
	}
	None
}
