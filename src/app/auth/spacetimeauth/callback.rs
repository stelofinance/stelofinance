use crate::auth::cookies::{
	COOKIE_STAUTH_MINT, COOKIE_STAUTH_NAME, COOKIE_STAUTH_NONCE, COOKIE_STAUTH_PKCE,
	COOKIE_STAUTH_PURPOSE, COOKIE_STAUTH_REDIRECT, COOKIE_STAUTH_STATE, MINT_FLASH_MAX_AGE_SECS,
	clear_stauth_oauth_cookies, cookies, get_cookie, is_valid_redirect,
};
use crate::auth::require_user;
use crate::auth::spacetimeauth::{MintFlash, SpacetimeAuthState};
use crate::stdb::{StdbState, acquire_user_db, create_user_app_ticket, replace_user_app_ticket};
use topcoat::{
	Result,
	context::{Cx, app_context},
	cookie::{Cookie, Cookies, time::Duration},
	router::{
		error::{SeeOther, see_other},
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

/// Complete SpacetimeAuth OIDC: ticket + one-shot app connect. Never sets BitAuth cookies.
#[route(GET)]
async fn callback(cx: &Cx) -> Result<SeeOther> {
	let user = require_user(cx).await?;
	let dest = get_cookie(cx, COOKIE_STAUTH_REDIRECT)
		.filter(|r| is_valid_redirect(r))
		.unwrap_or_else(|| "/app/me".to_owned());

	let fail = |msg: &str| {
		clear_stauth_oauth_cookies(cx);
		see_other(&with_error(&dest, msg))
	};

	let q = match query_params::<CallbackQuery>(cx) {
		Ok(q) => q,
		Err(_) => return Ok(fail("oauth")),
	};

	if q.error.is_some() {
		let desc = q.error_description.as_deref().unwrap_or("");
		eprintln!("spacetimeauth: provider error: {:?} {desc}", q.error);
		return Ok(fail("oauth"));
	}

	let Some(state) = q.state.as_deref() else {
		return Ok(fail("oauth"));
	};
	let Some(code) = q.code.as_deref() else {
		return Ok(fail("oauth"));
	};
	let expected = match get_cookie(cx, COOKIE_STAUTH_STATE) {
		Some(s) => s,
		None => return Ok(fail("oauth")),
	};
	if state != expected {
		return Ok(fail("oauth"));
	}
	let Some(nonce) = get_cookie(cx, COOKIE_STAUTH_NONCE) else {
		return Ok(fail("oauth"));
	};
	let Some(pkce) = get_cookie(cx, COOKIE_STAUTH_PKCE) else {
		return Ok(fail("oauth"));
	};
	let Some(name) = get_cookie(cx, COOKIE_STAUTH_NAME) else {
		return Ok(fail("name"));
	};
	let replace = get_cookie(cx, COOKIE_STAUTH_PURPOSE).as_deref() == Some("replace");

	let stauth = app_context::<SpacetimeAuthState>(cx);
	let Some(client) = stauth.client.as_ref() else {
		return Ok(fail("config"));
	};

	let tokens = match client.exchange_code(code, &pkce, &nonce).await {
		Ok(t) => t,
		Err(e) => {
			eprintln!("spacetimeauth: exchange failed: {e}");
			return Ok(fail("oauth"));
		}
	};

	let conn = match acquire_user_db(cx).await {
		Ok(c) => c,
		Err(_) => return Ok(fail("ticket")),
	};
	let ticket = if replace {
		replace_user_app_ticket(conn.get(), name.clone(), tokens.sub.clone()).await
	} else {
		create_user_app_ticket(conn.get(), name.clone(), tokens.sub.clone()).await
	};
	if let Err(e) = ticket {
		eprintln!("spacetimeauth: ticket: {e}");
		clear_stauth_oauth_cookies(cx);
		return Ok(see_other(&with_error(&dest, "ticket")));
	}

	let stdb = app_context::<StdbState>(cx);
	let identity_hex = match stdb.pool.acquire(&tokens.id_token).await {
		Ok(app_conn) => {
			let hex = app_conn.get().identity().to_hex().to_string();
			stdb.pool.invalidate_token(&tokens.id_token);
			Some(hex)
		}
		Err(e) => {
			eprintln!("spacetimeauth: fulfill connect: {e}");
			None
		}
	};

	let mint_id = stauth.mints.put(MintFlash {
		id_token: tokens.id_token,
		refresh_token: tokens.refresh_token,
		sub: tokens.sub,
		name,
		identity_hex,
		user: user.identity,
	});

	clear_stauth_oauth_cookies(cx);
	cookies(cx).add(
		Cookie::build((COOKIE_STAUTH_MINT, mint_id))
			.max_age(Duration::seconds(MINT_FLASH_MAX_AGE_SECS))
			.build(),
	);
	Ok(see_other(&dest))
}

fn with_error(dest: &str, code: &str) -> String {
	let sep = if dest.contains('?') { '&' } else { '?' };
	format!("{dest}{sep}appError={code}")
}
