//! `POST /logout` — Stelo session only. Does not call BitAuth end_session.

use crate::auth::cookies::{clear_auth_cookies, clear_oauth_cookies};
use topcoat::{
	Result,
	context::Cx,
	router::{
		error::{SeeOther, see_other},
		route,
	},
};

/// Drop Stelo cookies and send the browser home. BitAuth stays signed in.
#[route(POST)]
async fn logout(cx: &Cx) -> Result<SeeOther> {
	clear_auth_cookies(cx);
	clear_oauth_cookies(cx);
	Ok(see_other("/"))
}
