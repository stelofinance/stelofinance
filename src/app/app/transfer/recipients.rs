//! `GET /app/transfer/recipients` — directory search for the send form.

use super::markup::recipient_results_html;
use crate::auth::require_user;
use crate::stdb::{StdbError, acquire_user_db, search_directory};
use serde::Deserialize;
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchElements, Signals},
	router::{error::internal_server_error, route},
};

#[derive(Debug, Deserialize)]
struct SearchSignals {
	#[serde(default, rename = "fromId")]
	from_id: String,
	#[serde(default, rename = "fromLedgerId")]
	from_ledger_id: String,
	#[serde(default, rename = "recipientSearch")]
	recipient_search: String,
}

#[route(GET)]
async fn recipients(cx: &Cx, Signals(form): Signals<SearchSignals>) -> Result<PatchElements> {
	let _user = require_user(cx).await?;
	let term = form.recipient_search.trim();
	if term.is_empty() {
		return Ok(PatchElements::new(recipient_results_html(&[], false)));
	}
	let Ok(ledger_id) = form.from_ledger_id.trim().parse::<u64>() else {
		return Ok(PatchElements::new(recipient_results_html(&[], true)));
	};
	let exclude_id = form.from_id.trim().parse::<u64>().unwrap_or(0);

	let conn = acquire_user_db(cx).await?;
	let hits = search_directory(conn.get(), ledger_id, exclude_id, term)
		.await
		.map_err(|e| internal_server_error(StdbError(e)))?;
	Ok(PatchElements::new(recipient_results_html(&hits, true)))
}
