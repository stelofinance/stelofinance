//! `GET /app/activity/updates` — live `my_transfers` + `my_accounts` → Datastar patches.

use std::time::{SystemTime, UNIX_EPOCH};

use super::list::activity_body_html;
use crate::stdb::{ActivitySnapshot, LiveActivity, StdbError, acquire_user_db, unix_now_micros};
use futures_core::Stream;
use futures_util::stream;
use topcoat::{
	Result,
	context::Cx,
	datastar::PatchElements,
	router::{
		content::sse::{Event, KeepAlive, Sse, last_event_id},
		error::internal_server_error,
		query_params, route,
	},
};

#[allow(non_snake_case)]
#[query_params]
struct UpdatesQuery {
	accountId: Option<String>,
}

/// Long-lived SSE: subscribe for the connection lifetime; patch `#activity-body`.
///
/// First connect (no `Last-Event-Id`): seed an empty event id only — page SSR
/// is already current. Reconnect: one snapshot, then live changes.
///
/// Filter is client-side (`$accountId` + `data-show`); the snapshot is always
/// the full authorized set. `?accountId=` is only used to set initial
/// `display:none` on reconnect so the morph does not flash All.
#[route(GET)]
async fn updates(cx: &Cx) -> Result<Sse<impl Stream<Item = Result<Event>> + use<>>> {
	let reconnect = last_event_id(cx).is_some();
	let selected = query_params::<UpdatesQuery>(cx)
		.ok()
		.and_then(|q| q.accountId.clone())
		.and_then(|s| {
			let t = s.trim();
			if t.is_empty() {
				None
			} else {
				t.parse::<u64>().ok()
			}
		});
	let pooled = acquire_user_db(cx).await?;
	let (tx, rx) = tokio::sync::watch::channel(None);
	let live = LiveActivity::start(pooled, tx, reconnect)
		.map_err(|e| internal_server_error(StdbError(e)))?;

	let seed = if reconnect { None } else { Some(seed_event()) };

	let events = stream::unfold(
		(rx, live, seed, selected),
		|(mut rx, live, seed, selected)| async move {
			let event = if let Some(seed) = seed {
				seed
			} else {
				rx.changed().await.ok()?;
				let data = rx.borrow().clone()?;
				list_patch(&data, selected)
			};
			Some((Ok(event), (rx, live, None, selected)))
		},
	);

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

fn list_patch(data: &ActivitySnapshot, selected: Option<u64>) -> Event {
	PatchElements::new(activity_body_html(data, selected, unix_now_micros()))
		.id(event_id())
		.into()
}
