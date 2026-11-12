//! Post-login return targets (`/login?redirect=…` → OAuth cookie → callback).

/// Whether `url` is safe to send the browser to after login.
///
/// Accepts only a same-origin absolute path (with optional query) in its
/// percent-encoded wire form, as `uri(cx).path_and_query()` yields it:
///
/// - starts with `/`, but not `//` or `/\` (browsers treat both as
///   protocol-relative, i.e. another host);
/// - printable ASCII only. That rejects whitespace and control characters,
///   which browsers strip from URLs (`/\t/evil.com` → `//evil.com`);
/// - no `\` anywhere, and no `"`, `;` or `,` so the value is also a valid
///   cookie value for [`COOKIE_OAUTH_REDIRECT`](super::cookies::COOKIE_OAUTH_REDIRECT).
pub fn is_safe_redirect(url: &str) -> bool {
	let bytes = url.as_bytes();
	if bytes.first() != Some(&b'/') || matches!(bytes.get(1), Some(b'/' | b'\\')) {
		return false;
	}
	bytes
		.iter()
		.all(|&b| b.is_ascii_graphic() && !matches!(b, b'\\' | b'"' | b';' | b','))
}

/// `base?redirect=<target>` when `target` is safe, else just `base`.
pub fn with_redirect(base: &str, target: &str) -> String {
	if is_safe_redirect(target) {
		format!("{base}?redirect={}", encode_query_value(target))
	} else {
		base.to_owned()
	}
}

/// Percent-encode everything except RFC 3986 unreserved characters.
fn encode_query_value(s: &str) -> String {
	let mut out = String::with_capacity(s.len());
	for b in s.bytes() {
		if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
			out.push(b as char);
		} else {
			out.push_str(&format!("%{b:02X}"));
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn accepts_paths_and_queries() {
		assert!(is_safe_redirect("/app"));
		assert!(is_safe_redirect(
			"/app/request?ledgerid=1&amount=5&memo=hi%20there"
		));
	}

	#[test]
	fn rejects_other_hosts_and_odd_bytes() {
		for bad in [
			"",
			"app",
			"https://evil.com",
			"//evil.com",
			"/\\evil.com",
			"/\\/evil.com",
			"/\t/evil.com",
			"/app\n",
			"/a b",
			"/app;x",
			"/ä",
		] {
			assert!(!is_safe_redirect(bad), "{bad:?}");
		}
	}

	#[test]
	fn round_trips_through_query_encoding() {
		let target = "/app/request?ledgerid=1&memo=a%2Bb";
		assert_eq!(
			with_redirect("/login", target),
			"/login?redirect=/app/request%3Fledgerid%3D1%26memo%3Da%252Bb"
		);
		assert_eq!(with_redirect("/login", "//evil.com"), "/login");
	}
}
