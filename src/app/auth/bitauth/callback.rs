use crate::auth::{
	bitauth::BitAuth,
	cookies::{
		COOKIE_OAUTH_NONCE, COOKIE_OAUTH_PKCE, COOKIE_OAUTH_REDIRECT, COOKIE_OAUTH_STATE,
		COOKIE_REFRESH, COOKIE_TOKEN, REFRESH_MAX_AGE_SECS, TOKEN_MAX_AGE_SECS,
		clear_oauth_cookies, cookies, get_cookie, is_valid_redirect,
	},
};
use topcoat::{
	Result,
	context::{Cx, app_context},
	cookie::{Cookie, Cookies, time::Duration},
	router::{
		error::{SeeOther, bad_request, see_other},
		query_params, route,
	},
};

#[query_params]
struct CallbackQuery {
	code: Option<String>,
	state: Option<String>,
	error: Option<String>,
	error_description: Option<String>,
}

/// Complete BitAuth OIDC: exchange code, set token cookies, redirect home.
#[route(GET)]
async fn callback(cx: &Cx) -> Result<SeeOther> {
	let q =
		query_params::<CallbackQuery>(cx).map_err(|_| bad_request("invalid query parameters"))?;

	if let Some(err) = &q.error {
		let desc = q.error_description.as_deref().unwrap_or("");
		eprintln!("bitauth: provider error: {err} {desc}");
		clear_oauth_cookies(cx);
		return Err(bad_request("authentication failed").into());
	}

	let state = q
		.state
		.as_deref()
		.ok_or_else(|| bad_request("missing state"))?;
	let code = q
		.code
		.as_deref()
		.ok_or_else(|| bad_request("missing code"))?;

	let expected_state = get_cookie(cx, COOKIE_OAUTH_STATE)
		.ok_or_else(|| bad_request("missing oauth state cookie"))?;
	if state != expected_state {
		clear_oauth_cookies(cx);
		return Err(bad_request("invalid oauth state").into());
	}

	let nonce = get_cookie(cx, COOKIE_OAUTH_NONCE)
		.ok_or_else(|| bad_request("missing oauth nonce cookie"))?;
	let pkce =
		get_cookie(cx, COOKIE_OAUTH_PKCE).ok_or_else(|| bad_request("missing pkce cookie"))?;

	let tokens = app_context::<BitAuth>(cx)
		.exchange_code(code, &pkce, &nonce)
		.await
		.map_err(|e| {
			eprintln!("bitauth: exchange failed: {e}");
			bad_request("token exchange failed")
		})?;

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

	let mut dest = "/".to_owned();
	if let Some(redir) = get_cookie(cx, COOKIE_OAUTH_REDIRECT) {
		if is_valid_redirect(&redir) {
			dest = redir;
		}
	}
	clear_oauth_cookies(cx);

	Ok(see_other(&dest))
}
