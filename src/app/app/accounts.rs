use super::chrome::stub_page;
use crate::auth::require_user;
use topcoat::{Result, context::Cx, router::page, view::view};

/// `GET /app/accounts` — stub until H1.
#[page]
async fn accounts(cx: &Cx) -> Result {
	let _user = require_user(cx).await?;
	view! {
		stub_page(
			title: "Accounts",
			blurb: "Your wallets and balances will show up here. Create and manage debit accounts (coming next).",
		)
	}
}
