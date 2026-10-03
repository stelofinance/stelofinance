//! Session helpers: ensure a usable BitAuth ID token before STDB acquire.

use std::time::{SystemTime, UNIX_EPOCH};

use super::bitauth::{AuthTokens, BitAuth};
use super::cookies::{
	COOKIE_REFRESH, COOKIE_TOKEN, TOKEN_REFRESH_SKEW_SECS, get_cookie, set_auth_cookies,
};
use super::jwt_peek;
use topcoat::context::{Cx, app_context};

/// Result of [`ensure_bearer`]: ID token string ready for `pool.acquire`.
#[derive(Debug, Clone)]
pub struct Bearer {
	pub token: String,
	/// True when BitAuth refresh ran and cookies were rewritten this call.
	pub refreshed: bool,
}

#[derive(Debug)]
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

/// Return a bearer ID token for STDB, refreshing via BitAuth when near/past `exp`.
///
/// Does not talk to SpacetimeDB or einro. On successful refresh, rewrites
/// `bitauth_token` (and refresh cookie if the IdP rotated it; otherwise the
/// existing refresh cookie is re-applied so Max-Age stays consistent).
pub async fn ensure_bearer(cx: &Cx) -> Result<Bearer, EnsureBearerError> {
	let id_token = get_cookie(cx, COOKIE_TOKEN);
	let refresh_cookie = get_cookie(cx, COOKIE_REFRESH);

	let should_refresh = match id_token.as_deref() {
		None => true,
		Some(t) => token_needs_refresh(t),
	};

	if !should_refresh {
		return Ok(Bearer {
			token: id_token.expect("present when !should_refresh"),
			refreshed: false,
		});
	}

	let Some(refresh) = refresh_cookie else {
		// No refresh path. Use unexpired ID token if present; else login required.
		return match id_token {
			Some(t) if !token_is_past_exp(&t) => Ok(Bearer {
				token: t,
				refreshed: false,
			}),
			_ => Err(EnsureBearerError::Unauthenticated),
		};
	};

	let bitauth = app_context::<BitAuth>(cx);
	match bitauth.refresh(&refresh).await {
		Ok(tokens) => {
			let token = tokens.id_token.clone();
			// Preserve refresh cookie when IdP does not rotate it.
			let tokens = AuthTokens {
				id_token: tokens.id_token,
				refresh_token: tokens.refresh_token.or(Some(refresh)),
			};
			set_auth_cookies(cx, tokens);
			eprintln!("bitauth: refreshed id_token cookie");
			Ok(Bearer {
				token,
				refreshed: true,
			})
		}
		Err(e) => {
			eprintln!("bitauth: refresh failed: {e}");
			// Soft-fail while current ID token is still not past exp.
			if let Some(t) = id_token {
				if !token_is_past_exp(&t) {
					eprintln!("bitauth: using current id_token until exp");
					return Ok(Bearer {
						token: t,
						refreshed: false,
					});
				}
			}
			Err(EnsureBearerError::RefreshFailed(e))
		}
	}
}

fn token_needs_refresh(token: &str) -> bool {
	match jwt_peek::peek_exp_unix(token) {
		None => true,
		Some(exp) => {
			let now = unix_now();
			now.saturating_add(TOKEN_REFRESH_SKEW_SECS) >= exp
		}
	}
}

fn token_is_past_exp(token: &str) -> bool {
	match jwt_peek::peek_exp_unix(token) {
		None => false,
		Some(exp) => unix_now() >= exp,
	}
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
	use base64::Engine;
	use base64::engine::general_purpose::URL_SAFE_NO_PAD;

	fn token_with_exp(exp: u64) -> String {
		let payload = URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{exp}}}"#).as_bytes());
		format!("x.{payload}.y")
	}

	#[test]
	fn needs_refresh_when_within_skew() {
		let now = unix_now();
		// Inside skew window (exp in a few seconds).
		let soon = token_with_exp(now + 5);
		assert!(token_needs_refresh(&soon));
		// Comfortably beyond skew.
		let later = token_with_exp(now + TOKEN_REFRESH_SKEW_SECS + 600);
		assert!(!token_needs_refresh(&later));
		let past = token_with_exp(now.saturating_sub(10));
		assert!(token_needs_refresh(&past));
	}
}
