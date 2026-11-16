//! Directory search for the send form: the `transfer_recipients` shard.

use super::markup::{Recipient, recipient_hits};
use crate::domain::sendable_accounts;
use crate::stdb::{acquire_user_db, fetch_accounts_page, search_directory};
use topcoat::{
	Result,
	context::Cx,
	router::error::internal_server_error,
	runtime::{Signal, shard},
	view::{View, view},
};

/// Hits on the same ledger as the sending account `from_id`.
///
/// Shard endpoints skip the `/app` layout guard. The ledger comes from the
/// user's own account `from_id`, never from the client; an account the user
/// cannot send from yields no hits.
#[shard("/app/transfer/recipients")]
pub async fn transfer_recipients(
	cx: &Cx,
	query: String,
	from_id: u64,
	recipient_id: Signal<u64>,
	recipient_name: Signal<String>,
	recipient_addr: Signal<String>,
	recipient_label: Signal<String>,
	recipient_search: Signal<String>,
	error: Signal<String>,
) -> Result<impl View> {
	let term = query.trim();
	let hits = if term.is_empty() {
		None
	} else {
		let conn = acquire_user_db(cx).await?;
		let data = fetch_accounts_page(conn.get())
			.await
			.map_err(internal_server_error)?;
		let accounts = sendable_accounts(&data.accounts);
		Some(match accounts.iter().find(|a| a.account_id == from_id) {
			Some(from) => search_directory(conn.get(), from.ledger_id, from_id, term)
				.await
				.map_err(internal_server_error)?,
			None => Vec::new(),
		})
	};
	let recipient = Recipient {
		id: recipient_id,
		name: recipient_name,
		addr: recipient_addr,
		label: recipient_label,
		search: recipient_search,
	};
	Ok(view! { recipient_hits(hits: hits, recipient: recipient, error: error) })
}
