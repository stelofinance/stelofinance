use topcoat::{
	context::Cx,
	cookie::{Cookie, Cookies, SameSite, cookies as root_cookies},
};

pub const COOKIE_TOKEN: &str = "bitauth_token";
pub const COOKIE_REFRESH: &str = "bitauth_refresh_token";

pub const COOKIE_OAUTH_STATE: &str = "bitauth_oauth_state";
pub const COOKIE_OAUTH_NONCE: &str = "bitauth_oauth_nonce";
pub const COOKIE_OAUTH_PKCE: &str = "bitauth_oauth_pkce";
pub const COOKIE_OAUTH_REDIRECT: &str = "bitauth_oauth_redirect";

pub const OAUTH_ROUNDTRIP_MAX_AGE_SECS: i64 = 10 * 60;
pub const TOKEN_MAX_AGE_SECS: i64 = 30 * 60;
pub const REFRESH_MAX_AGE_SECS: i64 = 14 * 24 * 60 * 60;

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
