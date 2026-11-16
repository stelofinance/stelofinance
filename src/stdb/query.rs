//! One-shot view/table subscribe: wait for applied, collect, unsubscribe.

use std::sync::{Arc, Mutex};

use crate::module_bindings::{DbConnection, SubscriptionEventContext, SubscriptionHandle};
use spacetimedb_sdk::{DbContext, SubscriptionHandle as SubscriptionHandleExt};

type SubBuilder = <DbConnection as DbContext>::SubscriptionBuilder;

use super::call::{StdbError, call};
use super::connector::StdbConn;

/// Subscribe via the query builder, wait until applied, run `collect`, then unsubscribe.
///
/// `build` should chain `add_query(|q| q.from.…())` and finish with `.subscribe()`.
/// Rows are only valid inside `collect` (unsubscribe clears the client cache).
pub async fn subscribe_once<T, B, F>(conn: &StdbConn, build: B, collect: F) -> Result<T, StdbError>
where
	T: Send + 'static,
	B: FnOnce(SubBuilder) -> SubscriptionHandle,
	F: FnOnce(&SubscriptionEventContext) -> T + Send + 'static,
{
	let (reply, pending) = call("subscribe");
	// `on_applied` and `on_error` each need the reply; whichever fires first takes it.
	let slot = Arc::new(Mutex::new(Some((reply, collect))));

	let builder = conn
		.db()
		.subscription_builder()
		.on_applied({
			let slot = Arc::clone(&slot);
			move |ctx| {
				if let Some((reply, collect)) = slot.lock().unwrap().take() {
					reply.send_outcome(Ok(collect(ctx)));
				}
			}
		})
		.on_error(move |_ctx, err| {
			if let Some((reply, _)) = slot.lock().unwrap().take() {
				reply.send_outcome(Err(StdbError::internal(format!("subscribe error: {err}"))));
			}
		});
	let handle = build(builder);

	let outcome = pending.wait().await;
	let _ = handle.unsubscribe();
	outcome
}

/// Row callbacks also fire while a subscription applies (one insert per
/// existing row) and on unsubscribe. Only reducer / transaction events are real
/// changes worth pushing to the browser.
pub(super) fn is_live_row_change(
	event: &spacetimedb_sdk::Event<crate::module_bindings::Reducer>,
) -> bool {
	matches!(
		event,
		spacetimedb_sdk::Event::Reducer(_) | spacetimedb_sdk::Event::Transaction
	)
}
