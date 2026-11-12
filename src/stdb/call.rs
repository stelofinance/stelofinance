//! Waiting on SpacetimeDB callbacks from async code, and the error type for it.
//!
//! The SDK reports reducer, procedure and subscription results through
//! callbacks on its own thread. [`call`] pairs a [`Reply`] (moved into the
//! callback) with a [`Pending`] (awaited by the handler, with a timeout).

use std::fmt;
use std::time::Duration;

use tokio::sync::oneshot;

/// How long to wait for a reducer, procedure or subscription to answer.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(15);

const GENERIC_MESSAGE: &str = "Something went wrong. Please try again.";

/// A failed SpacetimeDB call.
#[derive(Debug, Clone)]
pub enum StdbError {
	/// The module answered `Err(message)`. Module messages are written for
	/// people, so pages may show them (often via `map_transfer_error`).
	Rejected(String),
	/// Transport, timeout or SDK failure. Logged once when created; pages show
	/// a generic message instead of the detail.
	Internal(String),
}

impl StdbError {
	/// Build (and log) an internal error.
	pub fn internal(detail: impl fmt::Display) -> Self {
		let detail = detail.to_string();
		eprintln!("stdb: {detail}");
		Self::Internal(detail)
	}

	/// Text safe to show to the user.
	pub fn user_message(&self) -> &str {
		match self {
			Self::Rejected(message) => message,
			Self::Internal(_) => GENERIC_MESSAGE,
		}
	}
}

impl fmt::Display for StdbError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Rejected(message) => write!(f, "rejected: {message}"),
			Self::Internal(detail) => f.write_str(detail),
		}
	}
}

impl std::error::Error for StdbError {}

/// Start waiting for one callback result. `name` labels timeouts in the logs.
pub(super) fn call<T>(name: &'static str) -> (Reply<T>, Pending<T>) {
	let (tx, rx) = oneshot::channel();
	(Reply(tx), Pending { name, rx })
}

/// Sending half; move it into the SDK callback.
pub(super) struct Reply<T>(oneshot::Sender<Result<T, StdbError>>);

impl<T> Reply<T> {
	/// Forward a reducer / procedure callback result
	/// (`Ok(Ok(v))`, module `Ok(Err(message))`, or SDK `Err(e)`).
	pub fn send<E: fmt::Display>(self, result: Result<Result<T, String>, E>) {
		self.send_outcome(match result {
			Ok(Ok(value)) => Ok(value),
			Ok(Err(message)) => Err(StdbError::Rejected(message)),
			Err(e) => Err(StdbError::internal(e)),
		});
	}

	pub fn send_outcome(self, outcome: Result<T, StdbError>) {
		let _ = self.0.send(outcome);
	}
}

/// Receiving half; await it in the handler.
pub(super) struct Pending<T> {
	name: &'static str,
	rx: oneshot::Receiver<Result<T, StdbError>>,
}

impl<T> Pending<T> {
	/// Wait after a reducer `*_then` call, failing fast if the send failed.
	pub async fn after_send<E: fmt::Display>(self, sent: Result<(), E>) -> Result<T, StdbError> {
		if let Err(e) = sent {
			return Err(StdbError::internal(format!("{} send: {e}", self.name)));
		}
		self.wait().await
	}

	pub async fn wait(self) -> Result<T, StdbError> {
		match tokio::time::timeout(CALL_TIMEOUT, self.rx).await {
			Ok(Ok(outcome)) => outcome,
			Ok(Err(_)) => Err(StdbError::internal(format!(
				"{}: callback dropped",
				self.name
			))),
			Err(_) => Err(StdbError::internal(format!("{} timed out", self.name))),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn rejected_message_is_shown() {
		let (reply, pending) = call::<()>("t");
		reply.send(Ok::<_, String>(Err("invalid balance".into())));
		let err = pending.wait().await.unwrap_err();
		assert_eq!(err.user_message(), "invalid balance");
	}

	#[tokio::test]
	async fn internal_detail_is_hidden() {
		let (reply, pending) = call::<()>("t");
		reply.send(Err::<Result<(), String>, _>("socket closed"));
		let err = pending.wait().await.unwrap_err();
		assert_eq!(err.user_message(), GENERIC_MESSAGE);
		assert_eq!(err.to_string(), "socket closed");
	}

	#[tokio::test]
	async fn dropped_reply_is_internal() {
		let (reply, pending) = call::<()>("t");
		drop(reply);
		assert!(matches!(pending.wait().await, Err(StdbError::Internal(_))));
	}
}
