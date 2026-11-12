//! Rules over the caller's account rows: roles, ordering, and which accounts
//! a send / deposit / withdraw / payment may use.

use crate::module_bindings::{AccountKind, LedgerKind, MyAccountRow, Role};

pub fn sort_accounts(accounts: &mut [MyAccountRow]) {
	accounts.sort_by(|a, b| {
		a.ledger_name
			.cmp(&b.ledger_name)
			.then(a.ledger_id.cmp(&b.ledger_id))
			.then(b.is_primary.cmp(&a.is_primary))
			.then(is_owner(b).cmp(&is_owner(a)))
			.then(display_name(a).cmp(display_name(b)))
			.then(a.account_id.cmp(&b.account_id))
	});
}

fn is_owner(a: &MyAccountRow) -> bool {
	matches!(a.role, Role::Owner)
}

fn display_name(a: &MyAccountRow) -> &str {
	a.label
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty())
		.unwrap_or(a.address.as_str())
}

/// True when the caller already has a primary debit on `ledger_id`.
pub fn has_primary_on_ledger(accounts: &[MyAccountRow], ledger_id: u64) -> bool {
	accounts
		.iter()
		.any(|a| a.ledger_id == ledger_id && a.is_primary)
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

/// Writable debits on `ledger_id`, excluding `exclude_id` (the request recipient).
pub fn pay_from_accounts(
	accounts: &[MyAccountRow],
	ledger_id: u64,
	exclude_id: u64,
) -> Vec<MyAccountRow> {
	sendable_accounts(accounts)
		.into_iter()
		.filter(|a| a.ledger_id == ledger_id && a.account_id != exclude_id)
		.collect()
}

/// Preferred from-account: explicit id, else primary, else first.
pub fn pick_sendable(accounts: &[MyAccountRow], requested: Option<u64>) -> Option<&MyAccountRow> {
	if let Some(id) = requested {
		if let Some(a) = accounts.iter().find(|a| a.account_id == id) {
			return Some(a);
		}
	}
	accounts.iter().find(|a| a.is_primary).or(accounts.first())
}

/// Write+ debit or credit — deposit/withdraw "your account" picker.
pub fn flow_accounts(accounts: &[MyAccountRow]) -> Vec<MyAccountRow> {
	accounts
		.iter()
		.filter(|a| role_rank(a.role) >= role_rank(Role::Write))
		.cloned()
		.collect()
}

/// Requested id if eligible, else primary debit, else first debit, else first.
pub fn pick_flow(accounts: &[MyAccountRow], requested: Option<u64>) -> Option<&MyAccountRow> {
	if let Some(id) = requested {
		if let Some(a) = accounts.iter().find(|a| a.account_id == id) {
			return Some(a);
		}
	}
	accounts
		.iter()
		.find(|a| a.is_primary && matches!(a.kind, AccountKind::Debit))
		.or_else(|| {
			accounts
				.iter()
				.find(|a| matches!(a.kind, AccountKind::Debit))
		})
		.or(accounts.first())
}

pub fn counterpart_kind(yours: AccountKind) -> AccountKind {
	match yours {
		AccountKind::Debit => AccountKind::Credit,
		AccountKind::Credit => AccountKind::Debit,
	}
}

/// Own Write+ credit on the same ledger as a debit, when there is exactly one.
pub fn own_issuer<'a>(
	accounts: &'a [MyAccountRow],
	yours: &MyAccountRow,
) -> Option<&'a MyAccountRow> {
	if !matches!(yours.kind, AccountKind::Debit) {
		return None;
	}
	let hits: Vec<&MyAccountRow> = accounts
		.iter()
		.filter(|a| {
			a.account_id != yours.account_id
				&& a.ledger_id == yours.ledger_id
				&& matches!(a.kind, AccountKind::Credit)
				&& role_rank(a.role) >= role_rank(Role::Write)
		})
		.collect();
	if hits.len() == 1 { Some(hits[0]) } else { None }
}

/// Accounts that share one ledger (asset), for grouped lists.
pub struct LedgerGroup<'a> {
	pub ledger_name: &'a str,
	pub ledger_kind: LedgerKind,
	pub accounts: Vec<&'a MyAccountRow>,
}

/// Group rows by ledger, keeping each group's rows in input order. Groups are
/// ordered by ledger name, then ledger id.
pub fn group_by_ledger<'a>(
	rows: impl IntoIterator<Item = &'a MyAccountRow>,
) -> Vec<LedgerGroup<'a>> {
	let mut groups: Vec<LedgerGroup<'a>> = Vec::new();
	for acc in rows {
		match groups
			.iter_mut()
			.find(|g| g.accounts[0].ledger_id == acc.ledger_id)
		{
			Some(g) => g.accounts.push(acc),
			None => groups.push(LedgerGroup {
				ledger_name: acc.ledger_name.as_str(),
				ledger_kind: acc.ledger_kind,
				accounts: vec![acc],
			}),
		}
	}
	groups.sort_by(|a, b| {
		a.ledger_name
			.cmp(b.ledger_name)
			.then(a.accounts[0].ledger_id.cmp(&b.accounts[0].ledger_id))
	});
	groups
}

/// The `'debit'` / `'credit'` token used for account kinds in Datastar signals.
pub fn kind_token(kind: AccountKind) -> &'static str {
	match kind {
		AccountKind::Debit => "debit",
		AccountKind::Credit => "credit",
	}
}

/// Inverse of [`kind_token`].
pub fn parse_kind(s: &str) -> Option<AccountKind> {
	match s.trim() {
		"debit" => Some(AccountKind::Debit),
		"credit" => Some(AccountKind::Credit),
		_ => None,
	}
}
