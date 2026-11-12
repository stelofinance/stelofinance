//! `GET /app/me/updates` — live apps + tickets patches.

use std::time::{SystemTime, UNIX_EPOCH};

use super::apps::my_apps_html;
use crate::auth::require_user;
use crate::stdb::apps::LiveMyApps;
use crate::stdb::{acquire_user_db, next_change};
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

#[route(GET)]
async fn updates(cx: &Cx) -> Result<Sse<impl Stream<Item = Result<Event>> + use<>>> {
	let _user = require_user(cx).await?;
	let reconnect = last_event_id(cx).is_some();
	let pooled = acquire_user_db(cx).await?;
	let (tx, rx) = tokio::sync::watch::channel(None);
	let live = LiveMyApps::start(pooled, tx, reconnect).map_err(internal_server_error)?;

	let seed = if reconnect { None } else { Some(seed_event()) };

	let events = stream::unfold((rx, live, seed), |(mut rx, live, seed)| async move {
		let event = if let Some(seed) = seed {
			seed
		} else {
			next_change(&mut rx, live.conn()).await?;
			let data = rx.borrow().clone()?;
			PatchElements::new(my_apps_html(&data))
				.id(event_id())
				.into()
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
