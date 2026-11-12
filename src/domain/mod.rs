//! Pure app rules over module rows (no I/O). `stdb` talks to SpacetimeDB;
//! pages and `stdb` both build on these.

pub mod accounts;
pub mod activity;
pub mod format;
pub mod transfer;

pub use accounts::{
	counterpart_kind, flow_accounts, has_primary_on_ledger, kind_token, own_issuer, parse_kind,
	parse_role, pay_from_accounts, pick_flow, pick_sendable, role_rank, sendable_accounts,
	sort_accounts,
};
pub use activity::{
	can_finalize_transfer, dedupe_and_sort_transfers, display_amount, selected_account_id,
	sender_receiver, timestamp_micros,
};
pub use format::{
	MAX_LEDGER_SCALE, day_heading, format_qty, format_rel_time, parse_qty, unix_now_micros,
};
pub use transfer::{
	MAX_MEMO_LEN, SearchScope, map_transfer_error, new_idempotency_key, normalize_memo,
	parse_recipient_term,
};
