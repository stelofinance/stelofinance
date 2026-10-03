//! `GET/POST /app/withdraw` — Redeem (debit → credit).

use super::issue::{Flow, FlowSignals, flow_create, flow_page};
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchSignals, Signals},
	router::{page, route},
	view::view,
};

/// `GET /app/withdraw` — player debit → issuer credit.
#[page]
async fn page(cx: &Cx) -> Result {
	let _ = cx;
	view! {
		flow_page(flow: Flow::Withdraw)
	}
}

#[route(POST)]
async fn create(cx: &Cx, Signals(form): Signals<FlowSignals>) -> Result<PatchSignals> {
	flow_create(cx, Flow::Withdraw, form).await
}
