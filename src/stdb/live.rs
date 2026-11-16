//! `LiveFeed`: SpacetimeDB subscription snapshots for `live!` regions.
//!
//! A `live!` body runs once per HTTP render and once per connected
//! (WebSocket) render. The Topcoat runtime restarts a run only when its
//! WebSocket closes: a body that returns or fails is never run again, and its
//! region silently stops updating. So on a connected render the feed itself
//! survives a SpacetimeDB disconnect or restart. It resubscribes on a fresh
//! pooled connection with backoff, and the new subscription's first snapshot
//! carries any change missed while it was down.
//!
//! Usage, inside a shard or component:
//!
//! ```ignore
//! Ok(live! {
//!     let mut feed = LiveFeed::new(LiveAccounts::start);
//!     loop {
//!         let rows = feed.next(cx).await?;
//!         let token = emit! { list_view(rows: rows) }?;
//!         if !connected(cx) {
//!             break Ok(token);
//!         }
//!     }
//! })
//! ```

use std::time::Duration;

use tokio::sync::watch;
use topcoat::{
	Result,
	context::{Cx, context},
	router::error::internal_server_error,
	runtime::connected_untracked,
};

use super::call::CALL_TIMEOUT;
use super::{StdbConn, StdbError, acquire_user_db};
use crate::auth::oidc::OidcAuth;
use crate::auth::{ensure_bearer, require_user};
use crate::einro::PooledConn;

const RETRY_MIN: Duration = Duration::from_millis(250);
const RETRY_MAX: Duration = Duration::from_secs(10);

/// A running subscription (one of the `Live*` structs). Dropping it
/// unsubscribes.
pub trait LiveSubscription: Send {
	/// The connection the subscription lives on.
	fn conn(&self) -> &StdbConn;
}

/// Latest-wins snapshots from a `Live*` subscription, resubscribing after a
/// SpacetimeDB disconnect on connected renders.
pub struct LiveFeed<T, L, F> {
	start: F,
	sub: Option<(L, watch::Receiver<Option<T>>)>,
	retry: Duration,
}

impl<T, L, F> LiveFeed<T, L, F>
where
	T: Clone + Send + Sync,
	L: LiveSubscription,
	F: FnMut(PooledConn<StdbConn>, watch::Sender<Option<T>>) -> Result<L, StdbError>,
{
	/// `start` subscribes on the given connection and publishes a snapshot on
	/// the sender: first the applied subscription, then one per change.
	pub fn new(start: F) -> Self {
		Self {
			start,
			sub: None,
			retry: RETRY_MIN,
		}
	}

	/// The next snapshot. The first call returns the current state.
	///
	/// On an HTTP render any failure is returned. On a connected render it
	/// only returns `Err` when the session is gone; SpacetimeDB failures are
	/// retried until a snapshot arrives.
	pub async fn next(&mut self, cx: &Cx) -> Result<T> {
		let connected = connected_untracked(cx);
		loop {
			match self.try_next(cx).await {
				Ok(Some(snapshot)) => {
					self.retry = RETRY_MIN;
					return Ok(snapshot);
				}
				Ok(None) => {
					// Subscription ended (connection dropped). Resubscribe.
					self.sub = None;
					if !connected {
						return Err(internal_server_error(StdbError::internal(
							"stdb connection closed before the snapshot",
						))
						.into());
					}
				}
				Err(e) => {
					self.sub = None;
					if !connected {
						return Err(e);
					}
					// A session that is gone will not come back on this socket.
					// The session was resolved once, from the handshake cookies,
					// and is memoized for the socket's lifetime, so check the
					// bearer's expiry itself: an expired token never reconnects.
					require_user(cx).await?;
					let expired = match ensure_bearer(cx).await {
						Ok(bearer) => context::<OidcAuth>(cx)
							.verify_id_token(&bearer.token)
							.is_err(),
						Err(_) => true,
					};
					if expired {
						return Err(internal_server_error(StdbError::internal(
							"live: session expired on an open socket",
						))
						.into());
					}
					eprintln!("live: resubscribing in {:?}: {e}", self.retry);
				}
			}
			tokio::time::sleep(self.retry).await;
			self.retry = (self.retry * 2).min(RETRY_MAX);
		}
	}

	/// `Ok(None)`: the subscription's connection closed.
	///
	/// A fresh subscription must deliver its first snapshot within
	/// `CALL_TIMEOUT`: the `Live*` `on_error` callbacks only log, so a failed
	/// subscribe would otherwise wait forever.
	async fn try_next(&mut self, cx: &Cx) -> Result<Option<T>> {
		let fresh = self.sub.is_none();
		if fresh {
			let conn = acquire_user_db(cx).await?;
			let (tx, rx) = watch::channel(None);
			let sub = (self.start)(conn, tx).map_err(internal_server_error)?;
			self.sub = Some((sub, rx));
		}
		let (sub, rx) = self.sub.as_mut().expect("subscribed above");
		let wait = async {
			loop {
				tokio::select! {
					changed = rx.changed() => {
						if changed.is_err() {
							return None;
						}
					}
					() = sub.conn().closed() => return None,
				}
				// `None` is the channel's initial value, never a snapshot.
				if let Some(snapshot) = rx.borrow_and_update().clone() {
					return Some(snapshot);
				}
			}
		};
		if !fresh {
			return Ok(wait.await);
		}
		match tokio::time::timeout(CALL_TIMEOUT, wait).await {
			Ok(snapshot) => Ok(snapshot),
			Err(_) => Err(internal_server_error(StdbError::internal(
				"live: no subscription snapshot in time",
			))
			.into()),
		}
	}
}
