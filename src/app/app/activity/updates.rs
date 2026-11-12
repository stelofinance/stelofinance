//! `GET /app/activity/updates` — live `my_transfers` + `my_accounts` → Datastar patches.

use std::time::{SystemTime, UNIX_EPOCH};

use super::list::activity_body_html;
use crate::domain::unix_now_micros;
use crate::stdb::{ActivitySnapshot, LiveActivity, acquire_user_db, next_change};
use futures_core::Stream;
use futures_util::stream;
use serde::Deserialize;
use topcoat::{
	Result,
	context::Cx,
	datastar::{PatchElements, Signals},
	router::{
		content::sse::{Event, KeepAlive, Sse, last_event_id},
		error::internal_server_error,
		route,
	},
};

/// The page's `$accountId` filter. Datastar sends signals on a GET as the
/// `datastar` query parameter, not as `?accountId=`.
#[derive(Deserialize)]
struct FilterSignals {
	#[serde(rename = "accountId", default)]
	account_id: String,
}

/// Long-lived SSE: subscribe for the connection lifetime; patch `#activity-body`.
///
/// First connect (no `Last-Event-Id`): seed an empty event id only — page SSR
/// is already current. Reconnect: one snapshot, then live changes.
///
/// Filter is client-side (`$accountId` + `data-show`); the snapshot is always
/// the full authorized set. `$accountId` (as sent when the stream opens) only
/// sets the initial `display:none`, so the morph does not flash All.
#[route(GET)]
async fn updates(
	cx: &Cx,
	filter: Option<Signals<FilterSignals>>,
) -> Result<Sse<impl Stream<Item = Result<Event>> + use<>>> {
	let reconnect = last_event_id(cx).is_some();
	let selected = filter.and_then(|Signals(f)| f.account_id.trim().parse::<u64>().ok());
	let pooled = acquire_user_db(cx).await?;
	let (tx, rx) = tokio::sync::watch::channel(None);
	let live = LiveActivity::start(pooled, tx, reconnect).map_err(internal_server_error)?;

	let seed = if reconnect { None } else { Some(seed_event()) };

	let events = stream::unfold(
		(rx, live, seed, selected),
		|(mut rx, live, seed, selected)| async move {
			let event = if let Some(seed) = seed {
				seed
			} else {
				next_change(&mut rx, live.conn()).await?;
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
