use super::chrome::stub_page;
use crate::auth::require_user;
use topcoat::{Result, context::Cx, router::page, view::view};

/// `GET /app/activity` — stub until transfer history (ex-H3 list) lands.
#[page]
async fn activity(cx: &Cx) -> Result {
	let _user = require_user(cx).await?;
	view! {
		stub_page(
			title: "Activity",
			blurb: "Transfer history across your accounts will live here (live updates planned).",
		)
	}
}
