use crate::auth::cookies::{
	COOKIE_STAUTH_NAME, COOKIE_STAUTH_NONCE, COOKIE_STAUTH_PKCE, COOKIE_STAUTH_PURPOSE,
	COOKIE_STAUTH_REDIRECT, COOKIE_STAUTH_STATE, OAUTH_ROUNDTRIP_MAX_AGE_SECS, cookies,
	is_valid_redirect,
};
use crate::auth::require_user;
use crate::auth::spacetimeauth::SpacetimeAuthState;
use topcoat::{
	Result,
	context::{Cx, app_context},
	cookie::{Cookie, Cookies, time::Duration},
	router::{error::redirect, page, query_params},
};

#[query_params]
struct LoginStartQuery {
	redirect: Option<String>,
	name: Option<String>,
	purpose: Option<String>,
}

/// Start SpacetimeAuth OIDC. Requires a BitAuth session. Does not touch BitAuth cookies.
#[page]
async fn login(cx: &Cx) -> Result {
	let _user = require_user(cx).await?;
	let stauth = app_context::<SpacetimeAuthState>(cx);
	let Some(client) = stauth.client.as_ref() else {
		return Err(redirect("/app/me?appError=config").into());
	};

	let q = query_params::<LoginStartQuery>(cx).ok();
	let name = q
		.as_ref()
		.and_then(|q| q.name.clone())
		.unwrap_or_default()
		.trim()
		.to_owned();
	if name.is_empty() || name.len() > 64 {
		let dest = q
			.as_ref()
			.and_then(|q| q.redirect.clone())
			.filter(|r| is_valid_redirect(r))
			.unwrap_or_else(|| "/app/me".to_owned());
		let sep = if dest.contains('?') { '&' } else { '?' };
		let url = format!("{dest}{sep}appError=name");
		return Err(redirect(url.as_str()).into());
	}
	let purpose = match q
		.as_ref()
		.and_then(|q| q.purpose.as_deref())
		.unwrap_or("create")
	{
		"replace" => "replace",
		_ => "create",
	};

	let start = client.auth_start();
	let jar = cookies(cx);
	let oauth_age = Duration::seconds(OAUTH_ROUNDTRIP_MAX_AGE_SECS);

	jar.add(
		Cookie::build((COOKIE_STAUTH_STATE, start.state))
			.max_age(oauth_age)
			.build(),
	);
	jar.add(
		Cookie::build((COOKIE_STAUTH_NONCE, start.nonce))
			.max_age(oauth_age)
			.build(),
	);
	jar.add(
		Cookie::build((COOKIE_STAUTH_PKCE, start.pkce_verifier))
			.max_age(oauth_age)
			.build(),
	);
	jar.add(
		Cookie::build((COOKIE_STAUTH_NAME, name))
			.max_age(oauth_age)
			.build(),
	);
	jar.add(
		Cookie::build((COOKIE_STAUTH_PURPOSE, purpose.to_owned()))
			.max_age(oauth_age)
			.build(),
	);

	if let Some(q) = q {
		if let Some(ref redir) = q.redirect {
			if is_valid_redirect(redir) {
				jar.add(
					Cookie::build((COOKIE_STAUTH_REDIRECT, redir.clone()))
						.max_age(oauth_age)
						.build(),
				);
			}
		}
	}

	Err(redirect(start.authorize_url.as_str()).into())
}
