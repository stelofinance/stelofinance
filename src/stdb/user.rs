//! One-shot `my_user` load for session resolution.

use spacetimedb_sdk::{DbContext, Table};

use super::connector::StdbConn;
use super::query::subscribe_once;
use crate::module_bindings::{MyUserRow, MyUserTableAccess, my_userQueryTableAccess};

/// Subscribe to `my_user`, wait for applied, clone the row, unsubscribe.
pub async fn fetch_my_user(conn: &StdbConn) -> Result<Option<MyUserRow>, String> {
	subscribe_once(
		conn,
		|b| b.add_query(|q| q.from.my_user()).subscribe(),
		|ctx| ctx.db().my_user().iter().next(),
	)
	.await
}
