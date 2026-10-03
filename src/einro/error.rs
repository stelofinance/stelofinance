use std::fmt;

/// Errors from [`super::IdentityPool::acquire`].
#[derive(Debug)]
pub enum PoolError {
	EmptyToken,
	Connect(String),
	Timeout(String),
	PoolFull,
	Disconnected,
}

impl fmt::Display for PoolError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::EmptyToken => write!(f, "empty auth token"),
			Self::Connect(e) => write!(f, "connect failed: {e}"),
			Self::Timeout(e) => write!(f, "timeout: {e}"),
			Self::PoolFull => write!(f, "connection pool full"),
			Self::Disconnected => write!(f, "connection disconnected"),
		}
	}
}

impl std::error::Error for PoolError {}
