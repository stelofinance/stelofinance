//! Unverified JWT claim peek for session skew / diagnostics only.
//! Not used as a pool key and not a security check — SpacetimeDB validates tokens at connect.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// Wall-clock `exp` claim (seconds since Unix epoch) if the token looks like a JWT
/// and the payload contains a numeric `exp`. No signature verification.
pub fn peek_exp_unix(token: &str) -> Option<u64> {
	let payload_b64 = token.split('.').nth(1)?;
	let bytes = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
	let payload = std::str::from_utf8(&bytes).ok()?;
	// Prefer `"exp":123` / `"exp": 123` without pulling in a JSON crate.
	let key = "\"exp\"";
	let idx = payload.find(key)?;
	let after = payload[idx + key.len()..].trim_start();
	let after = after.strip_prefix(':')?.trim_start();
	let end = after
		.find(|c: char| !c.is_ascii_digit())
		.unwrap_or(after.len());
	if end == 0 {
		return None;
	}
	after[..end].parse().ok()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn peeks_exp_from_unsigned_payload() {
		// {"exp":1700000000}
		let payload = URL_SAFE_NO_PAD.encode(br#"{"exp":1700000000}"#);
		let token = format!("x.{payload}.y");
		assert_eq!(peek_exp_unix(&token), Some(1_700_000_000));
	}
}
