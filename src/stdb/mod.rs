//! SpacetimeDB edge: config, einro adapter, helpers.

mod config;
mod connector;

pub use config::StdbConfig;
#[allow(unused_imports)] // used by app pages once STDB-backed routes land
pub use connector::{StdbConn, StdbConnector};

use crate::einro::{IdentityPool, PoolConfig};

/// App-context bundle: config + token-keyed connection pool.
#[derive(Clone)]
pub struct StdbState {
	pub config: StdbConfig,
	pub pool: std::sync::Arc<IdentityPool<StdbConnector>>,
}

impl StdbState {
	pub fn from_env() -> Self {
		let config = StdbConfig::from_env();
		let pool = IdentityPool::new(
			StdbConnector,
			config.host.clone(),
			config.database.clone(),
			PoolConfig::default(),
		);
		Self {
			config,
			pool: std::sync::Arc::new(pool),
		}
	}
}
