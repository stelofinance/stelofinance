//! One-shot `my_user` load for session resolution.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use spacetimedb_sdk::{DbContext, SubscriptionHandle, Table};
use tokio::task::spawn_blocking;

use super::connector::StdbConn;
use crate::module_bindings::{MyUserRow, MyUserTableAccess};

const SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Subscribe to `my_user`, wait for applied, clone the row, unsubscribe.
///
/// Uses the connection’s background message pump (`run_threaded`). Safe to call
/// from async request handlers via `spawn_blocking` for the wait.
pub async fn fetch_my_user(conn: &StdbConn) -> Result<Option<MyUserRow>, String> {
	type Outcome = Result<Option<MyUserRow>, String>;
	type Tx = Arc<Mutex<Option<std::sync::mpsc::SyncSender<Outcome>>>>;

	let (tx, rx) = std::sync::mpsc::sync_channel::<Outcome>(1);
	let tx: Tx = Arc::new(Mutex::new(Some(tx)));

	fn send(tx: &Tx, outcome: Outcome) {
		if let Some(sender) = tx.lock().unwrap().take() {
			let _ = sender.send(outcome);
		}
	}

	let db = conn.db();
	let handle = db
		.subscription_builder()
		.on_applied({
			let tx = Arc::clone(&tx);
			move |ctx| {
				let row = ctx.db().my_user().iter().next();
				send(&tx, Ok(row));
			}
		})
		.on_error({
			let tx = Arc::clone(&tx);
			move |_ctx, err| {
				send(&tx, Err(format!("my_user subscribe error: {err}")));
			}
		})
		.subscribe("SELECT * FROM my_user");

	let wait = spawn_blocking(move || {
		rx.recv_timeout(SUBSCRIBE_TIMEOUT)
			.map_err(|_| "my_user subscribe timed out".to_owned())
	})
	.await
	.map_err(|e| format!("my_user wait task: {e}"))?;

	// Drop the subscription even if applied already ran (cache cleanup).
	let _ = handle.unsubscribe();

	wait?
}
