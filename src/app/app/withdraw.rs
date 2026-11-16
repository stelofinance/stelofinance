//! `GET /app/withdraw` — Redeem (debit → credit).

use super::issue::{Flow, flow_page};
use topcoat::{
	Result,
	context::Cx,
	metadata::Metadata,
	router::page,
	view::{View, view},
};

/// `GET /app/withdraw` — player debit → issuer credit.
#[page(with(Metadata::new().title("Withdraw · Stelo Finance")))]
async fn page(cx: &Cx) -> Result<impl View> {
	Ok(view! { flow_page(flow: Flow::Withdraw) })
}
