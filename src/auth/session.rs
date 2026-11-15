//! Session helpers: ensure a usable OIDC ID token before STDB acquire.

use std::time::{SystemTime, UNIX_EPOCH};

use super::cookies::{
	COOKIE_REFRESH, COOKIE_TOKEN, TOKEN_REFRESH_SKEW_SECS, get_cookie, set_auth_cookies,
};
use super::oidc::{AuthTokens, OidcAuth, TokenRejected};
use crate::stdb::StdbState;
use topcoat::context::{Cx, context, memoize};

/// Result of [`ensure_bearer`]: ID token string ready for `pool.acquire`.
#[derive(Debug, Clone)]
pub struct Bearer {
	pub token: String,
}

#[derive(Debug, Clone)]
pub enum EnsureBearerError {
	/// No ID token and no usable refresh path.
	Unauthenticated,
	/// Refresh was attempted and failed (IdP error, no id_token, verify failed).
	RefreshFailed(String),
}

impl std::fmt::Display for EnsureBearerError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Unauthenticated => write!(f, "not authenticated"),
			Self::RefreshFailed(e) => write!(f, "token refresh failed: {e}"),
		}
	}
}

impl std::error::Error for EnsureBearerError {}

/// Return a bearer ID token for STDB, refreshing via the OIDC provider when near/past `exp`.
///
/// Does not talk to SpacetimeDB or einro. On successful refresh, rewrites
/// `oidc_token` (and refresh cookie if the IdP rotated it; otherwise the
/// existing refresh cookie is re-applied so Max-Age stays consistent).
///
/// Memoized per request: the session gate and `acquire_user_db` share one
/// answer, so a near-expiry request refreshes once instead of racing two
/// refresh-token exchanges (a rotating IdP would reject the second).
#[memoize(as_ref)]
pub async fn ensure_bearer(cx: &Cx) -> Result<Bearer, EnsureBearerError> {
	let oidc = context::<OidcAuth>(cx);
	let id_token = get_cookie(cx, COOKIE_TOKEN);
	let refresh_cookie = get_cookie(cx, COOKIE_REFRESH);

	// Only a token that passes signature / iss / aud / exp checks counts. A
	// forged or tampered cookie is treated like a missing one, so it never
	// reaches the pool.
	let current = id_token
		.as_deref()
		.and_then(|token| match oidc.verify_id_token(token) {
			Ok(exp) => Some((token.to_owned(), exp)),
			Err(TokenRejected::Expired) => None,
			Err(TokenRejected::Invalid(e)) => {
				eprintln!("oidc: rejected id_token cookie: {e}");
				None
			}
		});

	if let Some((token, exp)) = &current {
		if !needs_refresh(*exp, unix_now()) {
			return Ok(Bearer {
				token: token.clone(),
			});
		}
	}
	// Still valid, but inside the refresh window.
	let current = current.map(|(token, _)| token);

	let Some(refresh) = refresh_cookie else {
		// No refresh path. Use the still-valid ID token if there is one.
		return current
			.map(|token| Bearer { token })
			.ok_or(EnsureBearerError::Unauthenticated);
	};

	match oidc.refresh(&refresh).await {
		Ok(tokens) => {
			let token = tokens.id_token.clone();
			// Preserve refresh cookie when IdP does not rotate it.
			let tokens = AuthTokens {
				id_token: tokens.id_token,
				refresh_token: tokens.refresh_token.or(Some(refresh)),
			};
			set_auth_cookies(cx, tokens);
			eprintln!("oidc: refreshed id_token cookie");
			// The old token's pool slot would otherwise sit open until idle
			// eviction. In-flight handles (e.g. live streams) keep it until done.
			if let Some(old) = &id_token {
				context::<StdbState>(cx).pool.retire_token(old);
			}
			Ok(Bearer { token })
		}
		Err(e) => {
			eprintln!("oidc: refresh failed: {e}");
			// Soft-fail while the current ID token is still valid.
			if let Some(token) = current {
				eprintln!("oidc: using current id_token until exp");
				return Ok(Bearer { token });
			}
			Err(EnsureBearerError::RefreshFailed(e))
		}
	}
}

/// Refresh once `exp` is within [`TOKEN_REFRESH_SKEW_SECS`] of `now`.
fn needs_refresh(exp: u64, now: u64) -> bool {
	now.saturating_add(TOKEN_REFRESH_SKEW_SECS) >= exp
}

fn unix_now() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_secs())
		.unwrap_or(0)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn needs_refresh_within_skew() {
		let now = 1_000_000;
		assert!(needs_refresh(now + 5, now));
		assert!(needs_refresh(now, now));
		assert!(!needs_refresh(now + TOKEN_REFRESH_SKEW_SECS + 600, now));
	}
}
