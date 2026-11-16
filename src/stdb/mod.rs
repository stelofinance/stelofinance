//! SpacetimeDB edge: config, einro adapter, helpers.

pub mod account;
mod accounts;
mod activity;
mod admin;
pub mod apps;
mod call;
mod config;
mod connector;
mod live;
pub use live::{LiveFeed, LiveSubscription};
mod query;
mod user;

mod transfer;

pub use accounts::{LiveAccounts, create_user_account, fetch_accounts_page};
pub use activity::{ActivitySnapshot, LiveActivity, fetch_activity_page};
pub use admin::create_user_ledger;
pub use apps::{create_user_app_ticket, replace_user_app_ticket, search_apps};
pub use call::StdbError;
pub use config::StdbConfig;
pub use connector::{StdbConn, StdbConnector};
pub use transfer::{
	DirectoryHit, create_user_transfer, finalize_user_transfer, lookup_account, search_directory,
};
pub use user::fetch_my_user;

use std::time::Duration;

use crate::auth::{EnsureBearerError, ensure_bearer, require_user};
use crate::einro::{IdentityPool, PoolConfig, PoolError, PooledConn};
use topcoat::{
	Result,
	context::{Cx, context},
	router::error::internal_server_error,
};

/// Pooled STDB connection for the signed-in user (cookie → ensure_bearer → einro).
pub async fn acquire_user_db(cx: &Cx) -> Result<PooledConn<StdbConn>> {
	let _user = require_user(cx).await?;
	let bearer = match ensure_bearer(cx).await {
		Ok(b) => b,
		Err(EnsureBearerError::Unauthenticated) => {
			return Err(internal_server_error(StdbError::internal(
				"session token missing after auth gate",
			))
			.into());
		}
		Err(EnsureBearerError::RefreshFailed(e)) => {
			return Err(internal_server_error(StdbError::internal(e)).into());
		}
	};
	let stdb = context::<StdbState>(cx);
	stdb.pool
		.acquire(&bearer.token)
		.await
		.map_err(|e: PoolError| internal_server_error(StdbError::internal(e)).into())
}

/// App-context bundle: config + token-keyed connection pool + module HTTP client.
#[derive(Clone)]
pub struct StdbState {
	pub config: StdbConfig,
	pub pool: std::sync::Arc<IdentityPool<StdbConnector>>,
	pub http: reqwest::Client,
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
		let http = reqwest::Client::builder()
			.connect_timeout(Duration::from_secs(5))
			.timeout(Duration::from_secs(15))
			.redirect(reqwest::redirect::Policy::none())
			.build()
			.expect("reqwest client");
		let pool = std::sync::Arc::new(pool);
		pool.spawn_idle_sweeper(Duration::from_secs(60));
		Self { config, pool, http }
	}
}
