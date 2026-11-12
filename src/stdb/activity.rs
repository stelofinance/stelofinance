//! Activity page data: `my_transfers` + `my_accounts`, live subscribe.

use std::sync::Arc;

use crate::domain::{dedupe_and_sort_transfers, sort_accounts};
use crate::einro::PooledConn;
use crate::module_bindings::{
	MyAccountRow, MyAccountsTableAccess, MyTransferRow, MyTransfersTableAccess,
	SubscriptionHandle as ModuleSubHandle,
	my_accountsQueryTableAccess, my_transfersQueryTableAccess,
};
use spacetimedb_sdk::{DbContext, SubscriptionHandle, Table, TableWithPrimaryKey};

use super::connector::StdbConn;
use super::query::{is_live_row_change, subscribe_once};

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
