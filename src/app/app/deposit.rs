use super::chrome::stub_page;
use crate::auth::require_user;
use topcoat::{Result, context::Cx, router::page, view::view};

/// `GET /app/deposit` — UX sugar stub (issuer deposit flow later).
#[page]
async fn deposit(cx: &Cx) -> Result {
	let _user = require_user(cx).await?;
	view! {
		stub_page(
			title: "Deposit",
			blurb: "A clearer path to bring assets onto Stelo (issuer deposit). Placeholder for now.",
		)
	}
}
