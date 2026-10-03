use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::credentials::ConnectParams;
use super::error::PoolError;
use super::{Connection, Connector};

/// Pool sizing and timeouts.
#[derive(Debug, Clone)]
pub struct PoolConfig {
	pub max_connections: usize,
	/// Drop a slot when no handles remain and it has been idle this long.
	pub idle_ttl: Duration,
	pub connect_timeout: Duration,
}

impl Default for PoolConfig {
	fn default() -> Self {
		Self {
			max_connections: 512,
			idle_ttl: Duration::from_secs(180),
			connect_timeout: Duration::from_secs(15),
		}
	}
}

struct Slot<C> {
	conn: Arc<C>,
	last_used: Instant,
}

struct PoolInner<C> {
	/// Exact bearer token → live connection.
	slots: HashMap<String, Slot<C>>,
}

/// Shared handle to a pooled connection. Drop updates idle bookkeeping; does not close the socket.
pub struct PooledConn<C: Connection> {
	conn: Arc<C>,
	token: String,
	pool_touch: Option<Arc<Mutex<PoolInner<C>>>>,
}

impl<C: Connection> PooledConn<C> {
	pub fn get(&self) -> &C {
		&self.conn
	}

	/// How many Arc handles share this connection (including the pool’s).
	pub fn strong_count(&self) -> usize {
		Arc::strong_count(&self.conn)
	}

	/// Token this slot is keyed by (same string passed to [`IdentityPool::acquire`]).
	pub fn token(&self) -> &str {
		&self.token
	}
}

impl<C: Connection> std::ops::Deref for PooledConn<C> {
	type Target = C;
	fn deref(&self) -> &C {
		&self.conn
	}
}

impl<C: Connection> Drop for PooledConn<C> {
	fn drop(&mut self) {
		if let Some(inner) = self.pool_touch.take() {
			if let Ok(mut g) = inner.lock() {
				if let Some(slot) = g.slots.get_mut(&self.token) {
					slot.last_used = Instant::now();
				}
			}
		}
	}
}

/// Token-keyed connection pool.
///
/// Same exact `token` string → reuse. Different string (e.g. refreshed JWT) → separate slot.
/// No JWT parsing; SpacetimeDB validates on connect.
pub struct IdentityPool<C: Connector> {
	connector: C,
	uri: String,
	database: String,
	config: PoolConfig,
	inner: Arc<Mutex<PoolInner<C::Conn>>>,
}

impl<C: Connector> IdentityPool<C> {
	pub fn new(
		connector: C,
		uri: impl Into<String>,
		database: impl Into<String>,
		config: PoolConfig,
	) -> Self {
		Self {
			connector,
			uri: uri.into(),
			database: database.into(),
			config,
			inner: Arc::new(Mutex::new(PoolInner {
				slots: HashMap::new(),
			})),
		}
	}

	pub fn uri(&self) -> &str {
		&self.uri
	}

	pub fn database(&self) -> &str {
		&self.database
	}

	pub fn config(&self) -> &PoolConfig {
		&self.config
	}

	pub fn len(&self) -> usize {
		self.inner.lock().unwrap().slots.len()
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// Drop the slot for this token (if any).
	pub fn invalidate_token(&self, token: &str) {
		let mut g = self.inner.lock().unwrap();
		if let Some(slot) = g.slots.remove(token) {
			slot.conn.disconnect();
		}
	}

	/// Acquire a shared connection for this bearer token.
	pub async fn acquire(&self, token: &str) -> Result<PooledConn<C::Conn>, PoolError> {
		if token.is_empty() {
			return Err(PoolError::EmptyToken);
		}

		self.evict_idle();

		if let Some(pooled) = self.try_reuse(token) {
			return Ok(pooled);
		}

		let connect_fut = self.connector.connect(ConnectParams {
			uri: &self.uri,
			database: &self.database,
			token,
		});

		let conn = tokio::time::timeout(self.config.connect_timeout, connect_fut)
			.await
			.map_err(|_| PoolError::Timeout("connect".into()))?
			.map_err(|e| PoolError::Connect(e.to_string()))?;

		if !conn.is_active() {
			return Err(PoolError::Disconnected);
		}

		self.insert_and_checkout(token.to_owned(), conn)
	}

	fn try_reuse(&self, token: &str) -> Option<PooledConn<C::Conn>> {
		let mut g = self.inner.lock().unwrap();
		let Some(slot) = g.slots.get_mut(token) else {
			return None;
		};

		if !slot.conn.is_active() {
			let dead = g.slots.remove(token).unwrap();
			dead.conn.disconnect();
			return None;
		}

		slot.last_used = Instant::now();
		Some(PooledConn {
			conn: Arc::clone(&slot.conn),
			token: token.to_owned(),
			pool_touch: Some(Arc::clone(&self.inner)),
		})
	}

	fn insert_and_checkout(
		&self,
		token: String,
		conn: C::Conn,
	) -> Result<PooledConn<C::Conn>, PoolError> {
		let mut g = self.inner.lock().unwrap();

		// Race: another acquire inserted meanwhile.
		if let Some(slot) = g.slots.get_mut(&token) {
			if slot.conn.is_active() {
				conn.disconnect();
				slot.last_used = Instant::now();
				return Ok(PooledConn {
					conn: Arc::clone(&slot.conn),
					token,
					pool_touch: Some(Arc::clone(&self.inner)),
				});
			}
			let dead = g.slots.remove(&token).unwrap();
			dead.conn.disconnect();
		}

		while g.slots.len() >= self.config.max_connections {
			if !evict_one_idle(&mut g) {
				return Err(PoolError::PoolFull);
			}
		}

		let arc = Arc::new(conn);
		g.slots.insert(
			token.clone(),
			Slot {
				conn: Arc::clone(&arc),
				last_used: Instant::now(),
			},
		);

		Ok(PooledConn {
			conn: arc,
			token,
			pool_touch: Some(Arc::clone(&self.inner)),
		})
	}

	fn evict_idle(&self) {
		let mut g = self.inner.lock().unwrap();
		let now = Instant::now();
		let ttl = self.config.idle_ttl;
		let stale: Vec<String> = g
			.slots
			.iter()
			.filter(|(_, s)| {
				Arc::strong_count(&s.conn) == 1 && now.duration_since(s.last_used) >= ttl
			})
			.map(|(t, _)| t.clone())
			.collect();
		for token in stale {
			if let Some(slot) = g.slots.remove(&token) {
				slot.conn.disconnect();
			}
		}
	}
}

fn evict_one_idle<C: Connection>(g: &mut PoolInner<C>) -> bool {
	let victim = g
		.slots
		.iter()
		.filter(|(_, s)| Arc::strong_count(&s.conn) == 1)
		.min_by_key(|(_, s)| s.last_used)
		.map(|(t, _)| t.clone());
	if let Some(token) = victim {
		if let Some(slot) = g.slots.remove(&token) {
			slot.conn.disconnect();
			return true;
		}
	}
	false
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicUsize, Ordering};

	struct MockConn {
		active: Mutex<bool>,
	}

	impl Connection for MockConn {
		fn is_active(&self) -> bool {
			*self.active.lock().unwrap()
		}
		fn disconnect(&self) {
			*self.active.lock().unwrap() = false;
		}
	}

	struct MockConnector {
		opens: Arc<AtomicUsize>,
	}

	impl Connector for MockConnector {
		type Conn = MockConn;
		type Error = std::io::Error;

		async fn connect(&self, _params: ConnectParams<'_>) -> Result<Self::Conn, Self::Error> {
			self.opens.fetch_add(1, Ordering::SeqCst);
			Ok(MockConn {
				active: Mutex::new(true),
			})
		}
	}

	#[tokio::test]
	async fn reuses_same_token_string() {
		let opens = Arc::new(AtomicUsize::new(0));
		let pool = IdentityPool::new(
			MockConnector {
				opens: Arc::clone(&opens),
			},
			"http://127.0.0.1:3000",
			"db",
			PoolConfig::default(),
		);

		let a = pool.acquire("token-a").await.unwrap();
		let b = pool.acquire("token-a").await.unwrap();
		assert_eq!(opens.load(Ordering::SeqCst), 1);
		assert_eq!(a.token(), b.token());
		drop(a);
		drop(b);
		assert_eq!(pool.len(), 1);
	}

	#[tokio::test]
	async fn different_tokens_open_separately() {
		let opens = Arc::new(AtomicUsize::new(0));
		let pool = IdentityPool::new(
			MockConnector {
				opens: Arc::clone(&opens),
			},
			"http://127.0.0.1:3000",
			"db",
			PoolConfig::default(),
		);

		let _a = pool.acquire("token-a").await.unwrap();
		let _b = pool.acquire("token-b").await.unwrap();
		assert_eq!(opens.load(Ordering::SeqCst), 2);
		assert_eq!(pool.len(), 2);
	}

	#[tokio::test]
	async fn dead_slot_reconnects() {
		let opens = Arc::new(AtomicUsize::new(0));
		let pool = IdentityPool::new(
			MockConnector {
				opens: Arc::clone(&opens),
			},
			"http://127.0.0.1:3000",
			"db",
			PoolConfig::default(),
		);

		let a = pool.acquire("token-a").await.unwrap();
		a.disconnect();
		drop(a);
		// Slot still present but inactive → next acquire should open again.
		let _b = pool.acquire("token-a").await.unwrap();
		assert_eq!(opens.load(Ordering::SeqCst), 2);
	}

	#[tokio::test]
	async fn invalidate_token_drops_slot() {
		let pool = IdentityPool::new(
			MockConnector {
				opens: Arc::new(AtomicUsize::new(0)),
			},
			"http://127.0.0.1:3000",
			"db",
			PoolConfig::default(),
		);
		let c = pool.acquire("t").await.unwrap();
		drop(c);
		pool.invalidate_token("t");
		assert_eq!(pool.len(), 0);
	}
}
