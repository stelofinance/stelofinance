use crate::auth::{
	cookies::{
		COOKIE_OAUTH_NONCE, COOKIE_OAUTH_PKCE, COOKIE_OAUTH_REDIRECT, COOKIE_OAUTH_STATE,
		OAUTH_ROUNDTRIP_MAX_AGE_SECS, cookies,
	},
	oidc::OidcAuth,
	redirect::is_safe_redirect,
};
use topcoat::{
	Result,
	context::{Cx, context},
	cookie::{Cookie, Cookies, time::Duration},
	router::{error::redirect, page, query_params},
};

#[query_params]
struct LoginStartQuery {
	redirect: Option<String>,
}

/// Start the OIDC login: set CSRF/PKCE cookies and redirect to the IdP.
#[page]
async fn login(cx: &Cx) -> Result<()> {
	let start = context::<OidcAuth>(cx).auth_start();
	let jar = cookies(cx);
	let oauth_age = Duration::seconds(OAUTH_ROUNDTRIP_MAX_AGE_SECS);

	jar.add(
		Cookie::build((COOKIE_OAUTH_STATE, start.state))
			.max_age(oauth_age)
			.build(),
	);
	jar.add(
		Cookie::build((COOKIE_OAUTH_NONCE, start.nonce))
			.max_age(oauth_age)
			.build(),
	);
	jar.add(
		Cookie::build((COOKIE_OAUTH_PKCE, start.pkce_verifier))
			.max_age(oauth_age)
			.build(),
	);

	if let Ok(q) = query_params::<LoginStartQuery>(cx) {
		if let Some(ref redir) = q.redirect {
			if is_safe_redirect(redir) {
				jar.add(
					Cookie::build((COOKIE_OAUTH_REDIRECT, redir.clone()))
						.max_age(oauth_age)
						.build(),
				);
			}
		}
	}

	Err(redirect(start.authorize_url.as_str()).into())
}
