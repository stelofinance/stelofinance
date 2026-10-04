//! `GET/POST /app/deposit` — Issue (credit → debit).

use super::issue::{Flow, FlowSignals, flow_create, flow_page};
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchSignals, Signals},
	router::{page, route},
	view::{View, view},
};

/// `GET /app/deposit` — issuer credit → player debit.
#[page]
async fn page(cx: &Cx) -> Result<impl View> {
	let _ = cx;
	Ok(view! {
		flow_page(flow: Flow::Deposit)
	})
}

#[route(POST)]
async fn create(cx: &Cx, Signals(form): Signals<FlowSignals>) -> Result<PatchSignals> {
	flow_create(cx, Flow::Deposit, form).await
}
