//! `GET/POST /app/withdraw` — Redeem (debit → credit).

use super::issue::{Flow, FlowSignals, flow_create, flow_page};
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchSignals, Signals},
	metadata::Metadata,
	router::{page, route},
	view::{View, view},
};

/// `GET /app/withdraw` — player debit → issuer credit.
#[page(with(Metadata::new().title("Withdraw · Stelo Finance")))]
async fn page(cx: &Cx) -> Result<impl View> {
	Ok(view! { flow_page(flow: Flow::Withdraw) })
}

#[route(POST)]
async fn create(cx: &Cx, Signals(form): Signals<FlowSignals>) -> Result<PatchSignals> {
	flow_create(cx, Flow::Withdraw, form).await
}
