//! Accounts page data: `my_accounts` + public `ledger`, create, live subscribe.

use std::sync::Arc;

use crate::domain::sort_accounts;
use crate::einro::PooledConn;
use crate::module_bindings::{
	AccountKind, Ledger, LedgerTableAccess, MyAccountRow, MyAccountsTableAccess,
	SubscriptionHandle as ModuleSubHandle, create_account, ledgerQueryTableAccess,
	my_accountsQueryTableAccess,
};
use spacetimedb_sdk::{DbContext, SubscriptionHandle, Table, TableWithPrimaryKey};

use super::call::{StdbError, call};
use super::connector::StdbConn;
use super::query::{is_live_row_change, subscribe_once};

/// Snapshot for GET `/app/accounts` (list + create-form catalog).
pub struct AccountsPageData {
	pub accounts: Vec<MyAccountRow>,
	pub ledgers: Vec<Ledger>,
}

/// One-shot subscribe of `my_accounts` and the public `ledger` table.
pub async fn fetch_accounts_page(conn: &StdbConn) -> Result<AccountsPageData, StdbError> {
	subscribe_once(
		conn,
		|b| {
			b.add_query(|q| q.from.my_accounts())
				.add_query(|q| q.from.ledger())
				.subscribe()
		},
		|ctx| {
			let mut accounts: Vec<MyAccountRow> = ctx.db().my_accounts().iter().collect();
			sort_accounts(&mut accounts);
			let mut ledgers: Vec<Ledger> = ctx.db().ledger().iter().collect();
			ledgers.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
			AccountsPageData { accounts, ledgers }
		},
	)
	.await
}

/// Call `create_account`. First debit on a ledger should pass `is_primary`.
pub async fn create_user_account(
	conn: &StdbConn,
	ledger_id: u64,
	kind: AccountKind,
	address: Option<String>,
	label: Option<String>,
	is_primary: bool,
) -> Result<(), StdbError> {
	let (reply, pending) = call("create_account");
	let sent = conn.db().reducers.create_account_then(
		ledger_id,
		kind,
		address,
		label,
		is_primary,
		move |_, r| reply.send(r),
	);
	pending.after_send(sent).await
}

/// Live `my_accounts` subscription. Publishes a full snapshot on a watch slot
/// (latest-wins) for real row changes (`Reducer` / `Transaction`), not
/// subscribe-apply inserts. Multiple row callbacks in one transaction collapse
/// to one emitted render.
///
/// Drop unsubscribes and unregisters callbacks.
pub struct LiveAccounts {
	pooled: PooledConn<StdbConn>,
	sub: Option<ModuleSubHandle>,
	insert_id: Option<crate::module_bindings::MyAccountsInsertCallbackId>,
	update_id: Option<crate::module_bindings::MyAccountsUpdateCallbackId>,
	delete_id: Option<crate::module_bindings::MyAccountsDeleteCallbackId>,
}

impl super::LiveSubscription for LiveAccounts {
	fn conn(&self) -> &StdbConn {
		self.pooled.get()
	}
}

impl LiveAccounts {
	/// Subscribe and publish snapshots on `tx` (latest-wins).
	///
	/// The first value is the applied subscription (the current rows); each
	/// later value is a real change. Several row callbacks from one
	/// reducer/transaction overwrite the same slot, so the reader sees one.
	pub fn start(
		pooled: PooledConn<StdbConn>,
		tx: tokio::sync::watch::Sender<Option<Vec<MyAccountRow>>>,
	) -> Result<Self, StdbError> {
		let send_snapshot: Arc<dyn Fn(Vec<MyAccountRow>) + Send + Sync> = {
			let tx = tx.clone();
			Arc::new(move |mut rows: Vec<MyAccountRow>| {
				sort_accounts(&mut rows);
				let _ = tx.send(Some(rows));
			})
		};

		let insert_id = {
			let send = Arc::clone(&send_snapshot);
			pooled.get().db().db.my_accounts().on_insert(move |ctx, _| {
				if is_live_row_change(&ctx.event) {
					send(ctx.db().my_accounts().iter().collect());
				}
			})
		};
		let update_id = {
			let send = Arc::clone(&send_snapshot);
			pooled
				.get()
				.db()
				.db
				.my_accounts()
				.on_update(move |ctx, _, _| {
					if is_live_row_change(&ctx.event) {
						send(ctx.db().my_accounts().iter().collect());
					}
				})
		};
		let delete_id = {
			let send = Arc::clone(&send_snapshot);
			pooled.get().db().db.my_accounts().on_delete(move |ctx, _| {
				if is_live_row_change(&ctx.event) {
					send(ctx.db().my_accounts().iter().collect());
				}
			})
		};

		let handle = pooled
			.get()
			.db()
			.subscription_builder()
			.on_applied({
				let send = Arc::clone(&send_snapshot);
				move |ctx| send(ctx.db().my_accounts().iter().collect())
			})
			.on_error(move |_ctx, err| {
				eprintln!("accounts live subscribe error: {err}");
			})
			.add_query(|q| q.from.my_accounts())
			.subscribe();

		Ok(Self {
			pooled,
			sub: Some(handle),
			insert_id: Some(insert_id),
			update_id: Some(update_id),
			delete_id: Some(delete_id),
		})
	}
}

impl Drop for LiveAccounts {
	fn drop(&mut self) {
		let tables = &self.pooled.get().db().db;
		if let Some(id) = self.insert_id.take() {
			tables.my_accounts().remove_on_insert(id);
		}
		if let Some(id) = self.update_id.take() {
			tables.my_accounts().remove_on_update(id);
		}
		if let Some(id) = self.delete_id.take() {
			tables.my_accounts().remove_on_delete(id);
		}
		if let Some(sub) = self.sub.take() {
			let _ = sub.unsubscribe();
		}
	}
}
