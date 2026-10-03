//! Current-user resolution for `/app/*` (Topcoat `cx` helpers, not middleware).
//!
//! Flow: cookie → [`ensure_bearer`] → einro pool acquire → `my_user` view.
//! Call [`require_user`] from app pages/layouts; public pages use
//! [`current_user`] when a signed-in state is optional.

use spacetimedb_sdk::Identity;
use topcoat::{
	Result,
	context::{Cx, app_context, memoize},
	router::{
		error::{internal_server_error, redirect},
		uri,
	},
};

use super::cookies::is_valid_redirect;
use super::session::{EnsureBearerError, ensure_bearer};
use crate::einro::PoolError;
use crate::stdb::{StdbState, fetch_my_user};

/// Authenticated Stelo user for the current request (from STDB `my_user`).
#[derive(Debug, Clone)]
pub struct AppUser {
	pub identity: Identity,
	pub bitcraft_username: String,
	pub is_admin: bool,
}

#[derive(Debug, Clone)]
enum SessionError {
	/// No usable cookie / token rejected / no user row.
	Unauthenticated,
	/// STDB or pool failure that is not an auth rejection.
	Backend(String),
}

impl std::fmt::Display for SessionError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Unauthenticated => write!(f, "not authenticated"),
			Self::Backend(e) => write!(f, "session backend error: {e}"),
		}
	}
}

impl std::error::Error for SessionError {}

/// Resolve the signed-in user for this request (memoized).
///
/// Returns `None` when there is no session or the token is invalid. Backend
/// failures (pool full, timeout, subscribe error) surface as
/// [`SessionError::Backend`] so callers can 500 instead of sending the user
/// to login.
#[memoize]
async fn resolve_session(cx: &Cx) -> std::result::Result<AppUser, SessionError> {
	let bearer = match ensure_bearer(cx).await {
		Ok(b) => b,
		Err(EnsureBearerError::Unauthenticated) => return Err(SessionError::Unauthenticated),
		Err(EnsureBearerError::RefreshFailed(e)) => {
			// Refresh failed and ID token is not usable — treat as signed out.
			eprintln!("session: bearer refresh failed: {e}");
			return Err(SessionError::Unauthenticated);
		}
	};

	let stdb = app_context::<StdbState>(cx);
	let pooled = match stdb.pool.acquire(&bearer.token).await {
		Ok(c) => c,
		Err(PoolError::Connect(msg)) => {
			// SpacetimeDB rejected the token (or host unreachable as connect err).
			eprintln!("session: STDB connect failed: {msg}");
			return Err(SessionError::Unauthenticated);
		}
		Err(e) => return Err(SessionError::Backend(e.to_string())),
	};

	let row = match fetch_my_user(pooled.get()).await {
		Ok(row) => row,
		Err(e) => return Err(SessionError::Backend(e)),
	};

	match row {
		Some(u) => Ok(AppUser {
			identity: u.id,
			bitcraft_username: u.bitcraft_username,
			is_admin: u.is_admin,
		}),
		None => {
			// Valid STDB identity but no user row (should be rare after client_connected).
			eprintln!("session: my_user empty after connect");
			Err(SessionError::Unauthenticated)
		}
	}
}

/// Optional current user (signed-out → `None`). Backend errors still 500.
pub async fn current_user(cx: &Cx) -> Result<Option<&AppUser>> {
	match resolve_session(cx).await {
		Ok(user) => Ok(Some(user)),
		Err(SessionError::Unauthenticated) => Ok(None),
		Err(e @ SessionError::Backend(_)) => Err(internal_server_error(e.clone()).into()),
	}
}

/// Require a signed-in user or redirect to `/login?redirect=…`.
///
/// Call from any `/app/*` page, layout, or component. Memoized: layout + page
/// share one resolve per request.
pub async fn require_user(cx: &Cx) -> Result<&AppUser> {
	match resolve_session(cx).await {
		Ok(user) => Ok(user),
		Err(SessionError::Unauthenticated) => {
			let path = uri(cx).path();
			let login = if is_valid_redirect(path) {
				format!("/login?redirect={path}")
			} else {
				"/login".to_owned()
			};
			Err(redirect(&login).into())
		}
		Err(e @ SessionError::Backend(_)) => Err(internal_server_error(e.clone()).into()),
	}
}
