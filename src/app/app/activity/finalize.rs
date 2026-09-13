//! `POST /app/activity/finalize` — Confirm (post held) or Void pending Issue/Redeem.

use crate::auth::require_user;
use crate::stdb::{
	StdbError, acquire_user_db, can_finalize_transfer, fetch_activity_page, finalize_user_transfer,
	map_transfer_error,
};
use serde::{Deserialize, Serialize};
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchSignals, Signals},
	router::{error::internal_server_error, route},
};

#[derive(Debug, Deserialize)]
struct FinalizeSignals {
	#[serde(default, rename = "finalizeId")]
	finalize_id: String,
	#[serde(default, rename = "finalizeAction")]
	finalize_action: String,
}

#[derive(Serialize)]
struct FinalizePatch {
	#[serde(rename = "finalizeError")]
	finalize_error: String,
	#[serde(rename = "finalizeId")]
	finalize_id: String,
	#[serde(rename = "finalizeAction")]
	finalize_action: String,
}

/// `POST /app/activity/finalize` — issuer Write+ on the credit leg.
#[route(POST)]
async fn finalize(cx: &Cx, Signals(form): Signals<FinalizeSignals>) -> Result<PatchSignals> {
	let _user = require_user(cx).await?;
	let Ok(transfer_id) = form.finalize_id.trim().parse::<u64>() else {
		return patch_err("Pick a pending transfer.");
	};
	let void = match form.finalize_action.trim() {
		"void" => true,
		"confirm" => false,
		_ => return patch_err("Choose Confirm or Void."),
	};

	let conn = acquire_user_db(cx).await?;
	let data = match fetch_activity_page(conn.get()).await {
		Ok(d) => d,
		Err(e) => return Err(internal_server_error(StdbError(e)).into()),
	};
	let Some(tr) = data.transfers.iter().find(|t| t.id == transfer_id) else {
		return patch_err("Transfer not found.");
	};
	if !can_finalize_transfer(tr, &data.accounts) {
		return patch_err("You can't confirm this transfer.");
	}
	let held = tr.pending_amount.unwrap_or(0);
	if held == 0 {
		return patch_err("No pending amount to finalize.");
	}
	let amount = if void { 0 } else { held };

	match finalize_user_transfer(conn.get(), transfer_id, amount).await {
		Ok(()) => PatchSignals::json(&FinalizePatch {
			finalize_error: String::new(),
			finalize_id: String::new(),
			finalize_action: String::new(),
		}),
		Err(e) => patch_err(&map_transfer_error(&e)),
	}
}

fn patch_err(error: &str) -> Result<PatchSignals> {
	PatchSignals::json(&FinalizePatch {
		finalize_error: error.to_owned(),
		finalize_id: String::new(),
		finalize_action: String::new(),
	})
}
