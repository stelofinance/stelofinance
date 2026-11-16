//! `GET /app/deposit` — Issue (credit → debit).

use super::issue::{Flow, flow_page};
use topcoat::{
	Result,
	context::Cx,
	metadata::Metadata,
	router::page,
	view::{View, view},
};

/// `GET /app/deposit` — issuer credit → player debit.
#[page(with(Metadata::new().title("Deposit · Stelo Finance")))]
async fn page(cx: &Cx) -> Result<impl View> {
	Ok(view! { flow_page(flow: Flow::Deposit) })
}
