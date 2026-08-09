use super::chrome::stub_page;
use crate::auth::require_user;
use topcoat::{Result, context::Cx, router::page, view::view};

/// `GET /app/transfer` — send flow stub (H3 send half).
#[page]
async fn transfer(cx: &Cx) -> Result {
	let _user = require_user(cx).await?;
	view! {
		stub_page(
			title: "Send",
			blurb: "Send assets between accounts. Recipient search, memo, and idempotency will land here.",
		)
	}
}
