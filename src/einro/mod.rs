mod credentials;
mod error;
mod pool;

pub use credentials::ConnectParams;
#[allow(unused_imports)]
pub use error::PoolError;
pub use pool::{IdentityPool, PoolConfig, PooledConn};

use std::future::Future;

/// A live client session owned by the pool (shared via [`PooledConn`]).
pub trait Connection: Send + Sync + 'static {
	fn is_active(&self) -> bool;
	fn disconnect(&self);
}

/// Opens connections for the pool. Implemented by the app for its module bindings.
pub trait Connector: Send + Sync + 'static {
	type Conn: Connection;
	type Error: std::error::Error + Send + Sync + 'static;

	fn connect(
		&self,
		params: ConnectParams<'_>,
	) -> impl Future<Output = Result<Self::Conn, Self::Error>> + Send;
}
