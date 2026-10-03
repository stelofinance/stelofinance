//! Stelo adapter: einro `Connector` for generated `module_bindings::DbConnection`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use spacetimedb_sdk::{DbContext, Identity};
use tokio::task::spawn_blocking;

use crate::einro::{ConnectParams, Connection, Connector};
use crate::module_bindings::DbConnection;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Live STDB session: connection + background message pump.
pub struct StdbConn {
	conn: DbConnection,
	identity: Identity,
	_runner: std::thread::JoinHandle<()>,
}

impl StdbConn {
	pub fn db(&self) -> &DbConnection {
		&self.conn
	}

	/// Host-assigned Identity after connect (for logging / UI; not the pool key).
	pub fn identity(&self) -> Identity {
		self.identity
	}
}

impl Connection for StdbConn {
	fn is_active(&self) -> bool {
		DbContext::is_active(&self.conn)
	}

	fn disconnect(&self) {
		let _ = DbContext::disconnect(&self.conn);
	}
}

impl Drop for StdbConn {
	fn drop(&mut self) {
		let _ = DbContext::disconnect(&self.conn);
	}
}

/// Opens module connections with token auth; starts `run_threaded` pump.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdbConnector;

#[derive(Debug)]
pub struct StdbConnectError(pub String);

impl std::fmt::Display for StdbConnectError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0)
	}
}

impl std::error::Error for StdbConnectError {}

impl Connector for StdbConnector {
	type Conn = StdbConn;
	type Error = StdbConnectError;

	async fn connect(&self, params: ConnectParams<'_>) -> Result<Self::Conn, Self::Error> {
		let uri = params.uri.to_owned();
		let database = params.database.to_owned();
		let token = params.token.to_owned();

		spawn_blocking(move || connect_blocking(&uri, &database, &token))
			.await
			.map_err(|e| StdbConnectError(format!("connect task: {e}")))?
	}
}

fn connect_blocking(uri: &str, database: &str, token: &str) -> Result<StdbConn, StdbConnectError> {
	type Outcome = Result<Identity, String>;
	type Tx = Arc<Mutex<Option<std::sync::mpsc::SyncSender<Outcome>>>>;

	let (tx, rx) = std::sync::mpsc::sync_channel::<Outcome>(1);
	let tx: Tx = Arc::new(Mutex::new(Some(tx)));

	fn send(tx: &Tx, outcome: Outcome) {
		if let Some(sender) = tx.lock().unwrap().take() {
			let _ = sender.send(outcome);
		}
	}

	let conn = DbConnection::builder()
		.with_uri(uri)
		.with_database_name(database)
		.with_token(Some(token.to_owned()))
		.on_connect({
			let tx = Arc::clone(&tx);
			move |_conn, identity, _ws_token| {
				// Do not persist `_ws_token` over the BitAuth cookie.
				send(&tx, Ok(identity));
			}
		})
		.on_connect_error({
			let tx = Arc::clone(&tx);
			move |_ctx, err| {
				send(&tx, Err(format!("connect error: {err}")));
			}
		})
		.build()
		.map_err(|e| StdbConnectError(format!("build: {e}")))?;

	let runner = conn.run_threaded();

	let identity = rx
		.recv_timeout(CONNECT_TIMEOUT)
		.map_err(|_| StdbConnectError("connect timed out".into()))?
		.map_err(StdbConnectError)?;

	Ok(StdbConn {
		conn,
		identity,
		_runner: runner,
	})
}
