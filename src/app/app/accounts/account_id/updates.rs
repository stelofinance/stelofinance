//! `GET /app/accounts/{account_id}/updates` — live account home patches.

use std::time::{SystemTime, UNIX_EPOCH};

use super::AccountId;
use super::markup::{HomeChrome, account_home_html};
use crate::auth::require_user;
use crate::stdb::account::LiveAccountHome;
use crate::stdb::{StdbError, acquire_user_db};
use futures_core::Stream;
use futures_util::stream;
use topcoat::{
	Result,
	context::Cx,
	datastar::PatchElements,
	router::{
		content::sse::{Event, KeepAlive, Sse, last_event_id},
		error::internal_server_error,
		path_param, route,
	},
};

#[route(GET)]
async fn updates(cx: &Cx) -> Result<Sse<impl Stream<Item = Result<Event>> + use<>>> {
	let user = require_user(cx).await?;
	let account_id = *path_param::<AccountId>(cx)?;
	let reconnect = last_event_id(cx).is_some();
	let pooled = acquire_user_db(cx).await?;
	let (tx, rx) = tokio::sync::watch::channel(None);
	let live = LiveAccountHome::start(pooled, tx, account_id, reconnect)
		.map_err(|e| internal_server_error(StdbError(e)))?;

	let chrome = HomeChrome {
		caller_id: user.identity,
		caller_username: user.bitcraft_username.clone(),
	};

	let seed = if reconnect { None } else { Some(seed_event()) };

	let events = stream::unfold(
		(rx, live, seed, chrome),
		|(mut rx, live, seed, chrome)| async move {
			let event = if let Some(seed) = seed {
				seed
			} else {
				rx.changed().await.ok()?;
				let data = rx.borrow().clone()?;
				list_patch(&data, &chrome)
			};
			Some((Ok(event), (rx, live, None, chrome)))
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

fn list_patch(data: &crate::stdb::account::AccountHomeData, chrome: &HomeChrome) -> Event {
	PatchElements::new(account_home_html(data, chrome))
		.id(event_id())
		.into()
}
