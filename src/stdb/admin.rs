//! Platform-admin reducer calls (`User.is_admin`; module still `require_admin`).

use std::time::Duration;

use crate::module_bindings::{LedgerKind, create_ledger};
use tokio::task::spawn_blocking;

use super::connector::StdbConn;

const REDUCER_TIMEOUT: Duration = Duration::from_secs(15);

pub async fn create_user_ledger(
	conn: &StdbConn,
	name: String,
	scale: u8,
	kind: LedgerKind,
) -> Result<(), String> {
	let (tx, rx) = std::sync::mpsc::sync_channel(1);

	conn.db()
		.reducers
		.create_ledger_then(name, scale, kind, move |_ctx, result| {
			let outcome = match result {
				Ok(Ok(())) => Ok(()),
				Ok(Err(e)) => Err(e),
				Err(e) => Err(e.to_string()),
			};
			let _ = tx.send(outcome);
		})
		.map_err(|e| format!("create_ledger send: {e}"))?;

	let wait = spawn_blocking(move || {
		rx.recv_timeout(REDUCER_TIMEOUT)
			.map_err(|_| "create_ledger timed out".to_owned())
	})
	.await
	.map_err(|e| format!("create_ledger wait task: {e}"))?;
	wait?
}
