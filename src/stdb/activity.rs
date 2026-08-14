//! Activity page data: `my_transfers` + `my_accounts`, live subscribe.

use std::collections::HashSet;
use std::sync::Arc;

use crate::einro::PooledConn;
use crate::module_bindings::{
	MyAccountRow, MyAccountsTableAccess, MyTransferRow, MyTransfersTableAccess, Reducer,
	SubscriptionHandle as ModuleSubHandle, TransferKind, TransferState,
	my_accountsQueryTableAccess, my_transfersQueryTableAccess,
};
use spacetimedb_sdk::{DbContext, Event, SubscriptionHandle, Table, TableWithPrimaryKey};

use super::accounts::sort_accounts;
use super::connector::StdbConn;
use super::query::subscribe_once;

/// Snapshot for GET `/app/activity` and live `#activity-body` patches.
#[derive(Clone)]
pub struct ActivitySnapshot {
	pub accounts: Vec<MyAccountRow>,
	pub transfers: Vec<MyTransferRow>,
}

/// One-shot subscribe of `my_accounts` and `my_transfers`.
pub async fn fetch_activity_page(conn: &StdbConn) -> Result<ActivitySnapshot, String> {
	subscribe_once(
		conn,
		|b| {
			b.add_query(|q| q.from.my_accounts())
				.add_query(|q| q.from.my_transfers())
				.subscribe()
		},
		|ctx| {
			collect_activity(
				ctx.db().my_accounts().iter(),
				ctx.db().my_transfers().iter(),
			)
		},
	)
	.await
}

pub fn collect_activity(
	accounts: impl Iterator<Item = MyAccountRow>,
	transfers: impl Iterator<Item = MyTransferRow>,
) -> ActivitySnapshot {
	let mut accounts: Vec<MyAccountRow> = accounts.collect();
	sort_accounts(&mut accounts);
	let mut transfers: Vec<MyTransferRow> = transfers.collect();
	dedupe_and_sort_transfers(&mut transfers);
	ActivitySnapshot {
		accounts,
		transfers,
	}
}

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

fn is_live_row_change(event: &Event<Reducer>) -> bool {
	matches!(event, Event::Reducer(_) | Event::Transaction)
}

/// Live `my_transfers` + `my_accounts`. Latest-wins snapshots; ignore subscribe-apply.
///
/// `my_transfers` has no view `primary_key`, so the client only exposes insert/delete
/// (state changes arrive as delete+insert).
pub struct LiveActivity {
	pooled: PooledConn<StdbConn>,
	sub: Option<ModuleSubHandle>,
	acc_insert: Option<crate::module_bindings::MyAccountsInsertCallbackId>,
	acc_update: Option<crate::module_bindings::MyAccountsUpdateCallbackId>,
	acc_delete: Option<crate::module_bindings::MyAccountsDeleteCallbackId>,
	tr_insert: Option<crate::module_bindings::MyTransfersInsertCallbackId>,
	tr_delete: Option<crate::module_bindings::MyTransfersDeleteCallbackId>,
}

impl LiveActivity {
	pub fn start(
		pooled: PooledConn<StdbConn>,
		tx: tokio::sync::watch::Sender<Option<ActivitySnapshot>>,
		snapshot_on_applied: bool,
	) -> Result<Self, String> {
		let send_snapshot: Arc<dyn Fn(Vec<MyAccountRow>, Vec<MyTransferRow>) + Send + Sync> = {
			let tx = tx.clone();
			Arc::new(move |accounts, transfers| {
				let _ = tx.send(Some(collect_activity(
					accounts.into_iter(),
					transfers.into_iter(),
				)));
			})
		};

		let acc_insert = {
			let send = Arc::clone(&send_snapshot);
			pooled.get().db().db.my_accounts().on_insert(move |ctx, _| {
				if is_live_row_change(&ctx.event) {
					send(
						ctx.db().my_accounts().iter().collect(),
						ctx.db().my_transfers().iter().collect(),
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
							ctx.db().my_transfers().iter().collect(),
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
						ctx.db().my_transfers().iter().collect(),
					);
				}
			})
		};
		let tr_insert = {
			let send = Arc::clone(&send_snapshot);
			pooled
				.get()
				.db()
				.db
				.my_transfers()
				.on_insert(move |ctx, _| {
					if is_live_row_change(&ctx.event) {
						send(
							ctx.db().my_accounts().iter().collect(),
							ctx.db().my_transfers().iter().collect(),
						);
					}
				})
		};
		let tr_delete = {
			let send = Arc::clone(&send_snapshot);
			pooled
				.get()
				.db()
				.db
				.my_transfers()
				.on_delete(move |ctx, _| {
					if is_live_row_change(&ctx.event) {
						send(
							ctx.db().my_accounts().iter().collect(),
							ctx.db().my_transfers().iter().collect(),
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
							ctx.db().my_transfers().iter().collect(),
						);
					}
				}
			})
			.on_error(move |_ctx, err| {
				eprintln!("activity live subscribe error: {err}");
			})
			.add_query(|q| q.from.my_accounts())
			.add_query(|q| q.from.my_transfers())
			.subscribe();

		Ok(Self {
			pooled,
			sub: Some(handle),
			acc_insert: Some(acc_insert),
			acc_update: Some(acc_update),
			acc_delete: Some(acc_delete),
			tr_insert: Some(tr_insert),
			tr_delete: Some(tr_delete),
		})
	}
}

impl Drop for LiveActivity {
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
		if let Some(id) = self.tr_insert.take() {
			tables.my_transfers().remove_on_insert(id);
		}
		if let Some(id) = self.tr_delete.take() {
			tables.my_transfers().remove_on_delete(id);
		}
		if let Some(sub) = self.sub.take() {
			let _ = sub.unsubscribe();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::{selected_account_id, sender_receiver};
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
}
