use super::chrome::stub_page;
use crate::auth::require_user;
use topcoat::{Result, context::Cx, router::page, view::view};

/// `GET /app/withdraw` — UX sugar stub (issuer redeem flow later).
#[page]
async fn withdraw(cx: &Cx) -> Result {
	let _user = require_user(cx).await?;
	view! {
		stub_page(
			title: "Withdraw",
			blurb: "A clearer path to redeem assets back to BitCraft. Placeholder for now.",
		)
	}
}
