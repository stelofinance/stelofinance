//! `GET /app/accounts/updates` — live `my_accounts` → Datastar list patches.

use std::time::{SystemTime, UNIX_EPOCH};

use super::list::accounts_list_html;
use crate::module_bindings::MyAccountRow;
use crate::stdb::{LiveAccounts, StdbError, acquire_user_db};
use futures_core::Stream;
use futures_util::stream;
use topcoat::{
	Result,
	context::Cx,
	datastar::PatchElements,
	router::{
		content::sse::{Event, KeepAlive, Sse, last_event_id},
		error::internal_server_error,
		route,
	},
};

/// Long-lived SSE: subscribe for the connection lifetime; patch `#accounts-list`.
///
/// First connect (no `Last-Event-Id`): seed an empty event id only — page SSR
/// is already current. Reconnect: one snapshot, then live changes.
#[route(GET)]
async fn updates(cx: &Cx) -> Result<Sse<impl Stream<Item = Result<Event>> + use<>>> {
	let reconnect = last_event_id(cx).is_some();
	let pooled = acquire_user_db(cx).await?;
	// Latest-wins: two on_update callbacks from one transfer overwrite the
	// slot before this task reads, so the browser gets one fat morph.
	let (tx, rx) = tokio::sync::watch::channel(None);
	let live = LiveAccounts::start(pooled, tx, reconnect)
		.map_err(|e| internal_server_error(StdbError(e)))?;

	// First-connect: emit a seed event so the browser has a Last-Event-Id.
	// Reconnect: first payload is the applied snapshot from LiveAccounts.
	let seed = if reconnect { None } else { Some(seed_event()) };

	let events = stream::unfold((rx, live, seed), |(mut rx, live, seed)| async move {
		let event = if let Some(seed) = seed {
			seed
		} else {
			rx.changed().await.ok()?;
			let rows = rx.borrow().clone()?;
			list_patch(&rows)
		};
		Some((Ok(event), (rx, live, None)))
	});

	Ok(Sse::new(events).keep_alive(KeepAlive::new()))
}

fn event_id() -> String {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_millis().to_string())
		.unwrap_or_else(|_| "0".into())
}

fn seed_event() -> Event {
	PatchElements::new("").id(event_id()).into()
}

fn list_patch(rows: &[MyAccountRow]) -> Event {
	PatchElements::new(accounts_list_html(rows))
		.id(event_id())
		.into()
}
