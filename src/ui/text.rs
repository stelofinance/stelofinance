//! Escaping for hand-built markup, plus display labels for module enums.

use crate::module_bindings::{LedgerKind, Role};

/// Escape text for an HTML text node or a double/single-quoted attribute value.
pub fn escape_html(s: &str) -> String {
	let mut out = String::with_capacity(s.len());
	for c in s.chars() {
		match c {
			'&' => out.push_str("&amp;"),
			'<' => out.push_str("&lt;"),
			'>' => out.push_str("&gt;"),
			'"' => out.push_str("&quot;"),
			'\'' => out.push_str("&#39;"),
			c => out.push(c),
		}
	}
	out
}

/// A single-quoted JavaScript string literal (quotes included) for Datastar
/// expressions such as `data-signals`.
///
/// This only makes the value safe *as JS*. In a raw HTML string it must still
/// go through [`escape_html`]; `view!` attributes escape it already.
pub fn js_str(s: &str) -> String {
	let mut out = String::with_capacity(s.len() + 2);
	out.push('\'');
	for c in s.chars() {
		match c {
			'\\' => out.push_str("\\\\"),
			'\'' => out.push_str("\\'"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\u{2028}' => out.push_str("\\u2028"),
			'\u{2029}' => out.push_str("\\u2029"),
			c => out.push(c),
		}
	}
	out.push('\'');
	out
}

pub fn role_label(role: Role) -> &'static str {
	match role {
		Role::Read => "Read",
		Role::Write => "Write",
		Role::Admin => "Admin",
		Role::Owner => "Owner",
	}
}

pub fn ledger_kind_label(kind: LedgerKind) -> &'static str {
	match kind {
		LedgerKind::Physical => "In-game item",
		LedgerKind::Digital => "Digital",
		LedgerKind::Derivation => "Derivation",
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn escapes_html_specials() {
		assert_eq!(escape_html(r#"<a href="x">'&'</a>"#), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
	}

	#[test]
	fn js_str_quotes_and_escapes() {
		assert_eq!(js_str("it's"), r"'it\'s'");
		assert_eq!(js_str("a\\b\nc\r\u{2028}"), "'a\\\\b\\nc\\r\\u2028'");
	}
}
