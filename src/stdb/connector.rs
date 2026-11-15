//! Stelo adapter: einro `Connector` for generated `module_bindings::DbConnection`.

use std::sync::{Arc, Mutex};

use spacetimedb_sdk::{DbContext, Error as SdkError, Identity};
use tokio::sync::watch;
use tokio::task::spawn_blocking;

use super::call::{Reply, StdbError, call};
use crate::einro::{ConnectParams, Connection, Connector};
use crate::module_bindings::DbConnection;

/// Live STDB session: connection + background message pump.
///
/// Dropping it disconnects, including a half-open connection whose connect
/// timed out or failed.
pub struct StdbConn {
	conn: Arc<DbConnection>,
	closed: watch::Receiver<bool>,
	_runner: std::thread::JoinHandle<()>,
}

impl StdbConn {
	pub fn db(&self) -> &DbConnection {
		&self.conn
	}

	/// Resolves once the server connection has dropped.
	pub async fn closed(&self) {
		let mut closed = self.closed.clone();
		// An error means the sender (held by the SDK callbacks) is gone, which
		// also means the connection is over.
		let _ = closed.wait_for(|closed| *closed).await;
	}
}

impl Connection for StdbConn {
	fn is_active(&self) -> bool {
		DbContext::is_active(&*self.conn)
	}

	fn disconnect(&self) {
		let _ = DbContext::disconnect(&*self.conn);
	}
}

impl Drop for StdbConn {
	fn drop(&mut self) {
		let _ = DbContext::disconnect(&*self.conn);
	}
}

/// Opens module connections with token auth; starts the message pump.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdbConnector;

impl Connector for StdbConnector {
	type Conn = StdbConn;
	type Error = StdbError;

	async fn connect(&self, params: ConnectParams<'_>) -> Result<Self::Conn, Self::Error> {
		let uri = params.uri.to_owned();
		let database = params.database.to_owned();
		let token = params.token.to_owned();
		let (reply, pending) = call::<Identity>("connect");

		// `build` blocks on the websocket handshake. If the pool's timeout
		// drops this future first, the finished `StdbConn` is dropped (and
		// disconnected) when the blocking task returns.
		let conn = spawn_blocking(move || build_conn(&uri, &database, &token, reply))
			.await
			.map_err(|e| StdbError::internal(format!("connect task: {e}")))??;
		// On error or cancellation from here on, dropping `conn` disconnects.
		pending.wait().await?;
		Ok(conn)
	}
}

fn build_conn(
	uri: &str,
	database: &str,
	token: &str,
	reply: Reply<Identity>,
) -> Result<StdbConn, StdbError> {
	let reply = Arc::new(Mutex::new(Some(reply)));
	let take_reply = {
		let reply = Arc::clone(&reply);
		move || reply.lock().unwrap().take()
	};
	let (closed_tx, closed) = watch::channel(false);
	let closed_tx = Arc::new(closed_tx);

	let conn = DbConnection::builder()
		.with_uri(uri)
		.with_database_name(database)
		.with_token(Some(token.to_owned()))
		.on_connect({
			let take_reply = take_reply.clone();
			move |_conn, identity, _ws_token| {
				// Do not persist `_ws_token` over the OIDC token cookie.
				if let Some(reply) = take_reply() {
					reply.send_outcome(Ok(identity));
				}
			}
		})
		.on_connect_error(move |_ctx, err| {
			if let Some(reply) = take_reply() {
				reply.send_outcome(Err(StdbError::internal(format!("connect error: {err}"))));
			}
		})
		.on_disconnect({
			let closed_tx = Arc::clone(&closed_tx);
			move |_ctx, _err| {
				let _ = closed_tx.send(true);
			}
		})
		.build()
		.map_err(|e| StdbError::internal(format!("connect build: {e}")))?;

	let conn = Arc::new(conn);
	let runner = spawn_pump(Arc::clone(&conn), closed_tx)?;
	Ok(StdbConn {
		conn,
		closed,
		_runner: runner,
	})
}

/// Process messages until the connection ends, then mark it closed.
///
/// Replaces the SDK's `run_threaded`, which panics its thread on any error
/// other than a requested disconnect. That includes a connection the module's
/// `client_connected` refused, which `on_connect_error` has already reported.
fn spawn_pump(
	conn: Arc<DbConnection>,
	closed_tx: Arc<watch::Sender<bool>>,
) -> Result<std::thread::JoinHandle<()>, StdbError> {
	std::thread::Builder::new()
		.name("stdb-pump".to_owned())
		.spawn(move || {
			loop {
				match conn.advance_one_message_blocking() {
					Ok(()) => {}
					Err(SdkError::Disconnected | SdkError::FailedToConnect { .. }) => break,
					Err(e) => {
						eprintln!("stdb: connection ended: {e}");
						break;
					}
				}
			}
			let _ = closed_tx.send(true);
		})
		.map_err(|e| StdbError::internal(format!("spawn stdb pump: {e}")))
}
