//! `finalize_transfer` procedure — Confirm (post held) or Void pending Issue/Redeem.

use crate::auth::require_user;
use crate::domain::{can_finalize_transfer, map_transfer_error};
use crate::stdb::{acquire_user_db, fetch_activity_page, finalize_user_transfer};
use topcoat::{Result, context::Cx, runtime::procedure};

/// `POST /app/activity/finalize` — issuer Write+ on the credit leg.
/// `confirm == false` voids (posts 0).
#[procedure("/app/activity/finalize")]
pub async fn finalize_transfer(
	cx: &Cx,
	transfer_id: u64,
	confirm: bool,
) -> Result<std::result::Result<bool, String>> {
	if require_user(cx).await.is_err() {
		return Ok(Err("Your session expired. Reload the page.".into()));
	}
	let Ok(conn) = acquire_user_db(cx).await else {
		return Ok(Err("Your session expired. Reload the page.".into()));
	};
	let data = match fetch_activity_page(conn.get()).await {
		Ok(d) => d,
		Err(e) => {
			eprintln!("finalize: {e}");
			return Ok(Err("Couldn't load your transfers. Try again.".into()));
		}
	};
	let Some(tr) = data.transfers.iter().find(|t| t.id == transfer_id) else {
		return Ok(Err("Transfer not found.".into()));
	};
	if !can_finalize_transfer(tr, &data.accounts) {
		return Ok(Err("You can't confirm this transfer.".into()));
	}
	let held = tr.pending_amount.unwrap_or(0);
	if held == 0 {
		return Ok(Err("No pending amount to finalize.".into()));
	}
	let amount = if confirm { held } else { 0 };

	match finalize_user_transfer(conn.get(), transfer_id, amount).await {
		Ok(()) => Ok(Ok(true)),
		Err(e) => Ok(Err(map_transfer_error(e.user_message()))),
	}
}
