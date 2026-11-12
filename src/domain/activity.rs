//! Activity rules over transfer rows: dedupe/order, legs, and who may finalize.

use std::collections::HashSet;

use crate::module_bindings::{MyAccountRow, MyTransferRow, Role, TransferKind, TransferState};

use super::accounts::role_rank;

/// Keep one row per transfer id; newest `created_at` first.
pub fn dedupe_and_sort_transfers(transfers: &mut Vec<MyTransferRow>) {
	let mut seen = HashSet::new();
	transfers.retain(|t| seen.insert(t.id));
	transfers.sort_by(|a, b| {
		b.created_at
			.to_micros_since_unix_epoch()
			.cmp(&a.created_at.to_micros_since_unix_epoch())
			.then(b.id.cmp(&a.id))
	});
}

/// `?account=` only sticks if that id is one of the caller's accounts.
pub fn selected_account_id(raw: Option<&str>, accounts: &[MyAccountRow]) -> Option<u64> {
	let id = raw?.parse::<u64>().ok()?;
	accounts.iter().any(|a| a.account_id == id).then_some(id)
}

/// Sender / receiver account ids from double-entry legs + kind.
pub fn sender_receiver(kind: TransferKind, credit_id: u64, debit_id: u64) -> (u64, u64) {
	match kind {
		TransferKind::Liability => (debit_id, credit_id),
		TransferKind::Asset | TransferKind::Issue | TransferKind::Redeem => (credit_id, debit_id),
	}
}

/// Issuer (credit-kind) account that must Write to finalize Issue/Redeem.
pub fn issuer_account_id(kind: TransferKind, credit_id: u64, debit_id: u64) -> Option<u64> {
	let (sending, receiving) = sender_receiver(kind, credit_id, debit_id);
	match kind {
		TransferKind::Issue => Some(sending),
		TransferKind::Redeem => Some(receiving),
		TransferKind::Asset | TransferKind::Liability => None,
	}
}

pub fn can_finalize_transfer(tr: &MyTransferRow, accounts: &[MyAccountRow]) -> bool {
	if tr.state != TransferState::Pending {
		return false;
	}
	let Some(issuer_id) = issuer_account_id(tr.kind, tr.credit_account_id, tr.debit_account_id)
	else {
		return false;
	};
	accounts
		.iter()
		.any(|a| a.account_id == issuer_id && role_rank(a.role) >= role_rank(Role::Write))
}

/// Amount to show: pending while `Pending`, otherwise posted (fall back to pending).
pub fn display_amount(row: &MyTransferRow) -> u64 {
	match row.state {
		TransferState::Pending => row.pending_amount.unwrap_or(0),
		TransferState::Posted | TransferState::PostPending | TransferState::VoidPending => {
			row.posted_amount.or(row.pending_amount).unwrap_or(0)
		}
	}
}

pub fn timestamp_micros(ts: &spacetimedb_sdk::Timestamp) -> i64 {
	ts.to_micros_since_unix_epoch()
}

#[cfg(test)]
mod tests {
	use super::{issuer_account_id, selected_account_id, sender_receiver};
	use crate::module_bindings::TransferKind;

	#[test]
	fn sender_receiver_matrix() {
		assert_eq!(sender_receiver(TransferKind::Asset, 10, 20), (10, 20));
		assert_eq!(sender_receiver(TransferKind::Issue, 10, 20), (10, 20));
		assert_eq!(sender_receiver(TransferKind::Redeem, 10, 20), (10, 20));
		assert_eq!(sender_receiver(TransferKind::Liability, 10, 20), (20, 10));
	}

	#[test]
	fn unknown_account_query_is_all() {
		assert_eq!(selected_account_id(Some("9"), &[]), None);
		assert_eq!(selected_account_id(Some("nope"), &[]), None);
		assert_eq!(selected_account_id(None, &[]), None);
	}

	#[test]
	fn issuer_leg_is_not_always_credit_account_id() {
		// Issue: credit_account_id is the issuer (sending).
		assert_eq!(issuer_account_id(TransferKind::Issue, 10, 20), Some(10));
		// Redeem: credit_account_id is the player debit; issuer is receiving (debit_account_id).
		assert_eq!(issuer_account_id(TransferKind::Redeem, 10, 20), Some(20));
		assert_eq!(issuer_account_id(TransferKind::Asset, 10, 20), None);
	}
}
