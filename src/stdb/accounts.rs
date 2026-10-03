//! Accounts page data: `my_accounts` + public `ledger`, create, live subscribe.

use std::sync::Arc;
use std::time::Duration;

use crate::einro::PooledConn;
use crate::module_bindings::{
	AccountKind, Ledger, LedgerTableAccess, MyAccountRow, MyAccountsTableAccess, Reducer, Role,
	SubscriptionHandle as ModuleSubHandle, create_account, ledgerQueryTableAccess,
	my_accountsQueryTableAccess,
};
use spacetimedb_sdk::{DbContext, Event, SubscriptionHandle, Table, TableWithPrimaryKey};
use tokio::task::spawn_blocking;

use super::connector::StdbConn;
use super::query::subscribe_once;

const REDUCER_TIMEOUT: Duration = Duration::from_secs(15);

/// Snapshot for GET `/app/accounts` (list + create-form catalog).
pub struct AccountsPageData {
	pub accounts: Vec<MyAccountRow>,
	pub ledgers: Vec<Ledger>,
}

/// One-shot subscribe of `my_accounts` and the public `ledger` table.
pub async fn fetch_accounts_page(conn: &StdbConn) -> Result<AccountsPageData, String> {
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

/// Call `create_account`. First debit on a ledger should pass `is_primary`.
pub async fn create_user_account(
	conn: &StdbConn,
	ledger_id: u64,
	kind: AccountKind,
	address: Option<String>,
	label: Option<String>,
	is_primary: bool,
) -> Result<(), String> {
	let (tx, rx) = std::sync::mpsc::sync_channel(1);

	conn.db()
		.reducers
		.create_account_then(
			ledger_id,
			kind,
			address,
			None,
			label,
			is_primary,
			move |_ctx, result| {
				let outcome = match result {
					Ok(Ok(())) => Ok(()),
					Ok(Err(e)) => Err(e),
					Err(e) => Err(e.to_string()),
				};
				let _ = tx.send(outcome);
			},
		)
		.map_err(|e| format!("create_account send: {e}"))?;

	let wait = spawn_blocking(move || {
		rx.recv_timeout(REDUCER_TIMEOUT)
			.map_err(|_| "create_account timed out".to_owned())
	})
	.await
	.map_err(|e| format!("create_account wait task: {e}"))?;
	wait?
}

/// Live `my_accounts` subscription. Publishes a full snapshot on a watch slot
/// (latest-wins) for real row changes (`Reducer` / `Transaction`), not
/// subscribe-apply inserts. Multiple row callbacks in one transaction collapse
/// to one SSE patch.
///
/// Drop unsubscribes and unregisters callbacks.
pub struct LiveAccounts {
	pooled: PooledConn<StdbConn>,
	sub: Option<ModuleSubHandle>,
	insert_id: Option<crate::module_bindings::MyAccountsInsertCallbackId>,
	update_id: Option<crate::module_bindings::MyAccountsUpdateCallbackId>,
	delete_id: Option<crate::module_bindings::MyAccountsDeleteCallbackId>,
}

/// Row callbacks also fire for subscribe/unsubscribe apply. Those are not live updates.
fn is_live_row_change(event: &Event<Reducer>) -> bool {
	matches!(event, Event::Reducer(_) | Event::Transaction)
}

impl LiveAccounts {
	/// Subscribe and publish snapshots on `tx` (latest-wins).
	///
	/// Several row callbacks from one reducer/transaction overwrite the same
	/// slot; the SSE task reads once and sends a single Datastar patch.
	///
	/// `snapshot_on_applied`: publish the current cache when the subscription
	/// is applied (SSE reconnect / `Last-Event-Id`). First page load should pass
	/// `false` — SSR is already fresh.
	pub fn start(
		pooled: PooledConn<StdbConn>,
		tx: tokio::sync::watch::Sender<Option<Vec<MyAccountRow>>>,
		snapshot_on_applied: bool,
	) -> Result<Self, String> {
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
				move |ctx| {
					if snapshot_on_applied {
						send(ctx.db().my_accounts().iter().collect());
					}
				}
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
