//! One-shot view/table subscribe: wait for applied, collect, unsubscribe.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::module_bindings::{DbConnection, SubscriptionEventContext, SubscriptionHandle};
use spacetimedb_sdk::{DbContext, SubscriptionHandle as SubscriptionHandleExt};

type SubBuilder = <DbConnection as DbContext>::SubscriptionBuilder;
use tokio::task::spawn_blocking;

use super::connector::StdbConn;

const SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Subscribe via the query builder, wait until applied, run `collect`, then unsubscribe.
///
/// `build` should chain `add_query(|q| q.from.…())` and finish with `.subscribe()`.
/// Rows are only valid inside `collect` (unsubscribe clears the client cache).
pub async fn subscribe_once<T, B, F>(conn: &StdbConn, build: B, collect: F) -> Result<T, String>
where
	T: Send + 'static,
	B: FnOnce(SubBuilder) -> SubscriptionHandle,
	F: FnOnce(&SubscriptionEventContext) -> T + Send + 'static,
{
	type Outcome<T> = Result<T, String>;
	type Tx<T> = Arc<Mutex<Option<std::sync::mpsc::SyncSender<Outcome<T>>>>>;

	let (tx, rx) = std::sync::mpsc::sync_channel::<Outcome<T>>(1);
	let tx: Tx<T> = Arc::new(Mutex::new(Some(tx)));

	fn send<T>(tx: &Tx<T>, outcome: Outcome<T>) {
		if let Some(sender) = tx.lock().unwrap().take() {
			let _ = sender.send(outcome);
		}
	}

	let collect = Arc::new(Mutex::new(Some(collect)));

	let builder = conn
		.db()
		.subscription_builder()
		.on_applied({
			let tx = Arc::clone(&tx);
			let collect = Arc::clone(&collect);
			move |ctx| {
				let Some(collect) = collect.lock().unwrap().take() else {
					return;
				};
				send(&tx, Ok(collect(ctx)));
			}
		})
		.on_error({
			let tx = Arc::clone(&tx);
			move |_ctx, err| {
				send(&tx, Err(format!("subscribe error: {err}")));
			}
		});
	let handle = build(builder);

	let wait = spawn_blocking(move || {
		rx.recv_timeout(SUBSCRIBE_TIMEOUT)
			.map_err(|_| "subscribe timed out".to_owned())
	})
	.await
	.map_err(|e| format!("subscribe wait task: {e}"))?;

	let _ = handle.unsubscribe();
	wait?
}
