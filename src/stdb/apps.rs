//! Caller-owned apps + tickets, and global `app_search`.

use std::sync::Arc;
use std::time::Duration;

use crate::einro::PooledConn;
use crate::module_bindings::{
	AppSearchHit, MyAppRow, MyAppTicketRow, MyAppTicketsTableAccess, MyAppsTableAccess, Reducer,
	SubscriptionHandle as ModuleSubHandle, app_search, create_app_ticket,
	my_app_ticketsQueryTableAccess, my_appsQueryTableAccess, replace_app_ticket,
};
use spacetimedb_sdk::{DbContext, Event, SubscriptionHandle, Table, TableWithPrimaryKey};
use tokio::task::spawn_blocking;

use super::connector::StdbConn;
use super::query::subscribe_once;

const REDUCER_TIMEOUT: Duration = Duration::from_secs(15);

/// SSR / live snapshot of apps this caller created.
#[derive(Clone)]
pub struct MyAppsData {
	pub apps: Vec<MyAppRow>,
	pub tickets: Vec<MyAppTicketRow>,
}

pub async fn fetch_my_apps(conn: &StdbConn) -> Result<MyAppsData, String> {
	subscribe_once(
		conn,
		|b| {
			b.add_query(|q| q.from.my_apps())
				.add_query(|q| q.from.my_app_tickets())
				.subscribe()
		},
		|ctx| collect_my_apps(ctx.db().my_apps().iter(), ctx.db().my_app_tickets().iter()),
	)
	.await
}

fn collect_my_apps(
	apps: impl Iterator<Item = MyAppRow>,
	tickets: impl Iterator<Item = MyAppTicketRow>,
) -> MyAppsData {
	let mut apps: Vec<MyAppRow> = apps.collect();
	apps.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.to_hex().cmp(&b.id.to_hex())));
	let mut tickets: Vec<MyAppTicketRow> = tickets.collect();
	tickets.sort_by(|a, b| {
		a.expires_at
			.to_micros_since_unix_epoch()
			.cmp(&b.expires_at.to_micros_since_unix_epoch())
			.then(a.id.cmp(&b.id))
	});
	MyAppsData { apps, tickets }
}

fn map_reducer(result: Result<Result<(), String>, impl ToString>) -> Result<(), String> {
	match result {
		Ok(Ok(())) => Ok(()),
		Ok(Err(e)) => Err(e),
		Err(e) => Err(e.to_string()),
	}
}

async fn wait_reducer(
	start: impl FnOnce(std::sync::mpsc::SyncSender<Result<(), String>>) -> Result<(), String>,
) -> Result<(), String> {
	let (tx, rx) = std::sync::mpsc::sync_channel(1);
	start(tx)?;
	let wait = spawn_blocking(move || {
		rx.recv_timeout(REDUCER_TIMEOUT)
			.map_err(|_| "reducer timed out".to_owned())
	})
	.await
	.map_err(|e| format!("reducer wait task: {e}"))?;
	wait?
}

pub async fn create_user_app_ticket(
	conn: &StdbConn,
	name: String,
	sub: String,
) -> Result<(), String> {
	wait_reducer(|tx| {
		conn.db()
			.reducers
			.create_app_ticket_then(name, sub, move |_, r| {
				let _ = tx.send(map_reducer(r));
			})
			.map_err(|e| format!("create_app_ticket send: {e}"))
	})
	.await
}

pub async fn replace_user_app_ticket(
	conn: &StdbConn,
	name: String,
	sub: String,
) -> Result<(), String> {
	wait_reducer(|tx| {
		conn.db()
			.reducers
			.replace_app_ticket_then(name, sub, move |_, r| {
				let _ = tx.send(map_reducer(r));
			})
			.map_err(|e| format!("replace_app_ticket send: {e}"))
	})
	.await
}

/// Global case-insensitive app name prefix search.
pub async fn search_apps(conn: &StdbConn, term: &str) -> Result<Vec<AppSearchHit>, String> {
	let term = term.trim().to_owned();
	if term.is_empty() {
		return Ok(Vec::new());
	}
	let (tx, rx) = std::sync::mpsc::sync_channel(1);
	conn.db()
		.procedures
		.app_search_then(term, move |_ctx, result| {
			let outcome = match result {
				Ok(Ok(hits)) => Ok(hits),
				Ok(Err(e)) => Err(e),
				Err(e) => Err(e.to_string()),
			};
			let _ = tx.send(outcome);
		});
	let wait = spawn_blocking(move || {
		rx.recv_timeout(REDUCER_TIMEOUT)
			.map_err(|_| "app_search timed out".to_owned())
	})
	.await
	.map_err(|e| format!("app_search wait task: {e}"))?;
	wait?
}

fn is_live_row_change(event: &Event<Reducer>) -> bool {
	matches!(event, Event::Reducer(_) | Event::Transaction)
}

/// Live `my_apps` + `my_app_tickets`.
pub struct LiveMyApps {
	pooled: PooledConn<StdbConn>,
	sub: Option<ModuleSubHandle>,
	app_insert: Option<crate::module_bindings::MyAppsInsertCallbackId>,
	app_update: Option<crate::module_bindings::MyAppsUpdateCallbackId>,
	app_delete: Option<crate::module_bindings::MyAppsDeleteCallbackId>,
	tkt_insert: Option<crate::module_bindings::MyAppTicketsInsertCallbackId>,
	tkt_update: Option<crate::module_bindings::MyAppTicketsUpdateCallbackId>,
	tkt_delete: Option<crate::module_bindings::MyAppTicketsDeleteCallbackId>,
}

impl LiveMyApps {
	pub fn start(
		pooled: PooledConn<StdbConn>,
		tx: tokio::sync::watch::Sender<Option<MyAppsData>>,
		snapshot_on_applied: bool,
	) -> Result<Self, String> {
		let send_snapshot: Arc<dyn Fn(MyAppsData) + Send + Sync> = {
			let tx = tx.clone();
			Arc::new(move |data| {
				let _ = tx.send(Some(data));
			})
		};

		macro_rules! snap {
			($ctx:expr) => {
				collect_my_apps(
					$ctx.db().my_apps().iter(),
					$ctx.db().my_app_tickets().iter(),
				)
			};
		}

		let app_insert = {
			let send = Arc::clone(&send_snapshot);
			pooled.get().db().db.my_apps().on_insert(move |ctx, _| {
				if is_live_row_change(&ctx.event) {
					send(snap!(ctx));
				}
			})
		};
		let app_update = {
			let send = Arc::clone(&send_snapshot);
			pooled.get().db().db.my_apps().on_update(move |ctx, _, _| {
				if is_live_row_change(&ctx.event) {
					send(snap!(ctx));
				}
			})
		};
		let app_delete = {
			let send = Arc::clone(&send_snapshot);
			pooled.get().db().db.my_apps().on_delete(move |ctx, _| {
				if is_live_row_change(&ctx.event) {
					send(snap!(ctx));
				}
			})
		};
		let tkt_insert = {
			let send = Arc::clone(&send_snapshot);
			pooled
				.get()
				.db()
				.db
				.my_app_tickets()
				.on_insert(move |ctx, _| {
					if is_live_row_change(&ctx.event) {
						send(snap!(ctx));
					}
				})
		};
		let tkt_update = {
			let send = Arc::clone(&send_snapshot);
			pooled
				.get()
				.db()
				.db
				.my_app_tickets()
				.on_update(move |ctx, _, _| {
					if is_live_row_change(&ctx.event) {
						send(snap!(ctx));
					}
				})
		};
		let tkt_delete = {
			let send = Arc::clone(&send_snapshot);
			pooled
				.get()
				.db()
				.db
				.my_app_tickets()
				.on_delete(move |ctx, _| {
					if is_live_row_change(&ctx.event) {
						send(snap!(ctx));
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
						send(snap!(ctx));
					}
				}
			})
			.on_error(move |_ctx, err| {
				eprintln!("my apps live subscribe error: {err}");
			})
			.add_query(|q| q.from.my_apps())
			.add_query(|q| q.from.my_app_tickets())
			.subscribe();

		Ok(Self {
			pooled,
			sub: Some(handle),
			app_insert: Some(app_insert),
			app_update: Some(app_update),
			app_delete: Some(app_delete),
			tkt_insert: Some(tkt_insert),
			tkt_update: Some(tkt_update),
			tkt_delete: Some(tkt_delete),
		})
	}
}

impl Drop for LiveMyApps {
	fn drop(&mut self) {
		let tables = &self.pooled.get().db().db;
		if let Some(id) = self.app_insert.take() {
			tables.my_apps().remove_on_insert(id);
		}
		if let Some(id) = self.app_update.take() {
			tables.my_apps().remove_on_update(id);
		}
		if let Some(id) = self.app_delete.take() {
			tables.my_apps().remove_on_delete(id);
		}
		if let Some(id) = self.tkt_insert.take() {
			tables.my_app_tickets().remove_on_insert(id);
		}
		if let Some(id) = self.tkt_update.take() {
			tables.my_app_tickets().remove_on_update(id);
		}
		if let Some(id) = self.tkt_delete.take() {
			tables.my_app_tickets().remove_on_delete(id);
		}
		if let Some(sub) = self.sub.take() {
			let _ = sub.unsubscribe();
		}
	}
}
