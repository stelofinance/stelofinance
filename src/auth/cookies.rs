use crate::auth::bitauth::AuthTokens;
use topcoat::{
	context::Cx,
	cookie::{Cookie, Cookies, SameSite, cookies as root_cookies, time::Duration},
};

pub const COOKIE_TOKEN: &str = "bitauth_token";
pub const COOKIE_REFRESH: &str = "bitauth_refresh_token";

pub const COOKIE_OAUTH_STATE: &str = "bitauth_oauth_state";
pub const COOKIE_OAUTH_NONCE: &str = "bitauth_oauth_nonce";
pub const COOKIE_OAUTH_PKCE: &str = "bitauth_oauth_pkce";
pub const COOKIE_OAUTH_REDIRECT: &str = "bitauth_oauth_redirect";

pub const OAUTH_ROUNDTRIP_MAX_AGE_SECS: i64 = 10 * 60;
pub const TOKEN_MAX_AGE_SECS: i64 = 20 * 60;
pub const REFRESH_MAX_AGE_SECS: i64 = 14 * 24 * 60 * 60;
/// Refresh when ID token `exp` is within this many seconds (or already past).
///
/// SpacetimeDB accepts connect JWTs with ~60s post-`exp` leeway and does not
/// close live sockets when the connect token later expires, so a short skew is
/// enough: refresh near expiry so new acquires stay well inside that leeway.
pub const TOKEN_REFRESH_SKEW_SECS: u64 = 15;

pub fn cookies(cx: &Cx) -> impl Cookies {
	root_cookies(cx)
		.default_http_only(true)
		.default_same_site(SameSite::Lax)
		.default_path("/")
		.default_secure(std::env::var("ENV").is_ok_and(|v| v == "prod"))
}

pub fn get_cookie(cx: &Cx, name: &str) -> Option<String> {
	cookies(cx).get(name).map(|c| c.value().to_owned())
}

pub fn clear_cookie(cx: &Cx, name: &str) {
	cookies(cx).remove(Cookie::build((name.to_owned(), "")).build());
}

pub fn clear_auth_cookies(cx: &Cx) {
	clear_cookie(cx, COOKIE_TOKEN);
	clear_cookie(cx, COOKIE_REFRESH);
}

/// Write `bitauth_token` (+ optional rotated refresh) after login or refresh.
pub fn set_auth_cookies(cx: &Cx, tokens: AuthTokens) {
	let jar = cookies(cx);
	jar.add(
		Cookie::build((COOKIE_TOKEN, tokens.id_token))
			.max_age(Duration::seconds(TOKEN_MAX_AGE_SECS))
			.build(),
	);
	if let Some(refresh) = tokens.refresh_token {
		jar.add(
			Cookie::build((COOKIE_REFRESH, refresh))
				.max_age(Duration::seconds(REFRESH_MAX_AGE_SECS))
				.build(),
		);
	}
}

pub fn clear_oauth_cookies(cx: &Cx) {
	clear_cookie(cx, COOKIE_OAUTH_STATE);
	clear_cookie(cx, COOKIE_OAUTH_NONCE);
	clear_cookie(cx, COOKIE_OAUTH_PKCE);
	clear_cookie(cx, COOKIE_OAUTH_REDIRECT);
}

/// Relative path only: starts with `/`, no `//`, no `:`.
pub fn is_valid_redirect(url: &str) -> bool {
	url.starts_with('/') && !url.contains("//") && !url.contains(':')
}
