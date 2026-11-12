//! Transfer input rules: memo limits, idempotency keys, recipient search terms,
//! and user-facing text for module transfer errors.

/// Memo limit in **bytes**, matching the module's `MAX_MEMO_LEN` check
/// (`spacetimedb/src/transfers.rs`). Keep the two in sync.
pub const MAX_MEMO_LEN: usize = 64;

/// The memo was longer than [`MAX_MEMO_LEN`] bytes after trimming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoTooLong;

/// Trim a submitted memo. Blank becomes `None`; over the module limit is an error.
pub fn normalize_memo(raw: &str) -> Result<Option<String>, MemoTooLong> {
	let t = raw.trim();
	if t.is_empty() {
		Ok(None)
	} else if t.len() > MAX_MEMO_LEN {
		Err(MemoTooLong)
	} else {
		Ok(Some(t.to_owned()))
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchScope {
	Username,
	Address,
	Both,
}

/// Strip one leading `@` / `#` and decide which directory fields to match.
pub fn parse_recipient_term(raw: &str) -> Option<(SearchScope, String)> {
	let t = raw.trim();
	if t.is_empty() {
		return None;
	}
	if let Some(rest) = t.strip_prefix('@') {
		let term = rest.trim();
		if term.is_empty() {
			return None;
		}
		Some((SearchScope::Username, term.to_ascii_uppercase()))
	} else if let Some(rest) = t.strip_prefix('#') {
		let term = rest.trim();
		if term.is_empty() {
			return None;
		}
		Some((SearchScope::Address, term.to_ascii_uppercase()))
	} else {
		Some((SearchScope::Both, t.to_ascii_uppercase()))
	}
}

pub fn new_idempotency_key() -> String {
	// 128 random bits as 32 hex chars (same shape as the old timestamp key).
	// A timestamp could collide across users/instances and is guessable.
	let mut bytes = [0u8; 16];
	getrandom::fill(&mut bytes).expect("OS entropy source unavailable");
	bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn map_transfer_error(err: &str) -> String {
	match err {
		"invalid balance" => "Not enough available balance.".to_owned(),
		"sender is receiver" => "Pick a different recipient.".to_owned(),
		"incompatible account ledgers" => "That account is a different asset.".to_owned(),
		"invalid quantity" => "Enter an amount greater than zero.".to_owned(),
		"idempotency key conflict" => {
			"This send was already submitted with different details. Refresh and try again."
				.to_owned()
		}
		"insufficient account permission" => {
			"You don't have permission to complete this transfer.".to_owned()
		}
		"write access required on sender or receiver" => {
			"You need Write on the issuer or the wallet.".to_owned()
		}
		"transfer is not pending" => "That transfer is no longer pending.".to_owned(),
		"only issue/redeem pending transfers can be finalized" => {
			"Only deposits and withdrawals can be confirmed.".to_owned()
		}
		other => other.to_owned(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn prefix_scope() {
		assert_eq!(
			parse_recipient_term("@Nin"),
			Some((SearchScope::Username, "NIN".into()))
		);
		assert_eq!(
			parse_recipient_term("#hex"),
			Some((SearchScope::Address, "HEX".into()))
		);
		assert_eq!(
			parse_recipient_term("  nin  "),
			Some((SearchScope::Both, "NIN".into()))
		);
		assert!(parse_recipient_term("@").is_none());
		assert!(parse_recipient_term("").is_none());
	}
}
