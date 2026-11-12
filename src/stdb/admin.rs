//! Platform-admin reducer calls (`User.is_admin`; module still `require_admin`).

use crate::module_bindings::{LedgerKind, create_ledger};

use super::call::{StdbError, call};
use super::connector::StdbConn;

pub async fn create_user_ledger(
	conn: &StdbConn,
	name: String,
	scale: u8,
	kind: LedgerKind,
) -> Result<(), StdbError> {
	let (reply, pending) = call("create_ledger");
	let sent = conn
		.db()
		.reducers
		.create_ledger_then(name, scale, kind, move |_, r| reply.send(r));
	pending.after_send(sent).await
}
