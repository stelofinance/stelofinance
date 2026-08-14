//! SpacetimeDB edge: config, einro adapter, helpers.

pub mod account;
mod accounts;
mod activity;
mod config;
mod connector;
mod format;
mod query;
mod user;

mod transfer;

pub use accounts::{LiveAccounts, create_user_account, fetch_accounts_page, has_primary_on_ledger};
pub use activity::{
	ActivitySnapshot, LiveActivity, display_amount, fetch_activity_page, selected_account_id,
	sender_receiver, timestamp_micros,
};
pub use config::StdbConfig;
pub use connector::{StdbConn, StdbConnector};
pub use format::{day_heading, format_qty, format_rel_time, parse_qty, unix_now_micros};
pub use transfer::{
	DirectoryHit, create_user_transfer, map_transfer_error, new_idempotency_key, search_directory,
	sendable_accounts,
};
pub use user::fetch_my_user;

use crate::auth::{EnsureBearerError, ensure_bearer, require_user};
use crate::einro::{IdentityPool, PoolConfig, PoolError, PooledConn};
use topcoat::{
	Result,
	context::{Cx, app_context},
	router::error::internal_server_error,
};

/// Small error wrapper so `internal_server_error` has a `std::error::Error`.
#[derive(Debug)]
pub struct StdbError(pub String);

impl std::fmt::Display for StdbError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.0)
	}
}

impl std::error::Error for StdbError {}

impl From<String> for StdbError {
	fn from(s: String) -> Self {
		Self(s)
	}
}

impl From<&str> for StdbError {
	fn from(s: &str) -> Self {
		Self(s.to_owned())
	}
}

/// Pooled STDB connection for the signed-in user (cookie → ensure_bearer → einro).
pub async fn acquire_user_db(cx: &Cx) -> Result<PooledConn<StdbConn>> {
	let _user = require_user(cx).await?;
	let bearer = match ensure_bearer(cx).await {
		Ok(b) => b,
		Err(EnsureBearerError::Unauthenticated) => {
			return Err(internal_server_error(StdbError::from(
				"session token missing after auth gate",
			))
			.into());
		}
		Err(EnsureBearerError::RefreshFailed(e)) => {
			return Err(internal_server_error(StdbError(e)).into());
		}
	};
	let stdb = app_context::<StdbState>(cx);
	stdb.pool
		.acquire(&bearer.token)
		.await
		.map_err(|e: PoolError| internal_server_error(StdbError(e.to_string())).into())
}

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
