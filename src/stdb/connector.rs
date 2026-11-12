//! Stelo adapter: einro `Connector` for generated `module_bindings::DbConnection`.

use std::sync::{Arc, Mutex};

use spacetimedb_sdk::{DbContext, Identity};
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
	conn: DbConnection,
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

	let conn = DbConnection::builder()
		.with_uri(uri)
		.with_database_name(database)
		.with_token(Some(token.to_owned()))
		.on_connect({
			let take_reply = take_reply.clone();
			move |_conn, identity, _ws_token| {
				// Do not persist `_ws_token` over the BitAuth cookie.
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
		.on_disconnect(move |_ctx, _err| {
			let _ = closed_tx.send(true);
		})
		.build()
		.map_err(|e| StdbError::internal(format!("connect build: {e}")))?;

	let runner = conn.run_threaded();
	Ok(StdbConn {
		conn,
		closed,
		_runner: runner,
	})
}
