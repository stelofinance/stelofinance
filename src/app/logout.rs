//! `POST /logout` — Stelo session only. Does not call BitAuth end_session.

use crate::auth::cookies::{COOKIE_TOKEN, clear_auth_cookies, clear_oauth_cookies, get_cookie};
use crate::stdb::StdbState;
use topcoat::{
	Result,
	context::{Cx, context},
	router::{
		error::{SeeOther, see_other},
		route,
	},
};

/// Drop Stelo cookies and send the browser home. BitAuth stays signed in.
#[route(POST)]
async fn logout(cx: &Cx) -> Result<SeeOther> {
	// Close this session's SpacetimeDB connection now instead of at idle eviction.
	if let Some(token) = get_cookie(cx, COOKIE_TOKEN) {
		context::<StdbState>(cx).pool.invalidate_token(&token);
	}
	clear_auth_cookies(cx);
	clear_oauth_cookies(cx);
	Ok(see_other("/"))
}
