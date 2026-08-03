use crate::auth::{
	bitauth::BitAuth,
	cookies::{COOKIE_TOKEN, clear_auth_cookies, clear_oauth_cookies, get_cookie},
};
use topcoat::{
	Result,
	context::{Cx, app_context},
	router::{
		error::{SeeOther, redirect, see_other},
		route,
	},
};

/// Clear BitAuth cookies and optionally redirect to BitAuth end_session.
#[route(GET)]
async fn logout(cx: &Cx) -> Result<SeeOther> {
	let id_token = get_cookie(cx, COOKIE_TOKEN);
	clear_auth_cookies(cx);
	clear_oauth_cookies(cx);

	let bitauth = app_context::<BitAuth>(cx);
	if let Some(url) = bitauth.end_session_url(id_token.as_deref()) {
		// Temporary redirect so the browser follows to the IdP logout URL.
		return Err(redirect(url.as_str()).into());
	}

	Ok(see_other("/"))
}
