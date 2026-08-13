//! Display formatting for STDB quantities (asset scale → human decimal).

/// Format a raw integer amount using the ledger's decimal `scale`.
///
/// `123450` at scale `2` → `"1,234.50"`. Trailing fractional zeros are kept so
/// balances line up visually (matches “always show scale digits”).
pub fn format_qty(amount: u64, scale: u8) -> String {
	if scale == 0 {
		return group_thousands(amount);
	}

	let div = 10u64.saturating_pow(u32::from(scale));
	let whole = amount / div;
	let frac = amount % div;
	format!(
		"{}.{:0width$}",
		group_thousands(whole),
		frac,
		width = usize::from(scale)
	)
}

/// Parse a human quantity into the ledger's raw integer.
///
/// Accepts optional commas in the whole part (`1,234.50`). Fractional digits
/// must not exceed `scale`. Empty, negative, or non-numeric input is an error.
pub fn parse_qty(s: &str, scale: u8) -> Result<u64, String> {
	let raw = s.trim().replace(',', "");
	if raw.is_empty() {
		return Err("Enter an amount.".to_owned());
	}
	if raw.starts_with('-') || raw.starts_with('+') {
		return Err("Enter a valid amount.".to_owned());
	}

	let (whole_s, frac_s) = match raw.split_once('.') {
		Some((w, f)) => (w, Some(f)),
		None => (raw.as_str(), None),
	};
	let whole_s = if whole_s.is_empty() { "0" } else { whole_s };

	if !whole_s.chars().all(|c| c.is_ascii_digit()) {
		return Err("Enter a valid amount.".to_owned());
	}
	if let Some(f) = frac_s {
		if f.is_empty() || !f.chars().all(|c| c.is_ascii_digit()) {
			return Err("Enter a valid amount.".to_owned());
		}
		if scale == 0 {
			return Err("This asset doesn't use decimals.".to_owned());
		}
		if f.len() > usize::from(scale) {
			return Err("Too many decimal places.".to_owned());
		}
	}

	let whole: u64 = whole_s
		.parse()
		.map_err(|_| "Amount is too large.".to_owned())?;
	if scale == 0 {
		return Ok(whole);
	}

	let mul = 10u64.saturating_pow(u32::from(scale));
	let frac = match frac_s {
		Some(f) => {
			let mut padded = f.to_string();
			while padded.len() < usize::from(scale) {
				padded.push('0');
			}
			padded
				.parse::<u64>()
				.map_err(|_| "Enter a valid amount.".to_owned())?
		}
		None => 0,
	};
	whole
		.checked_mul(mul)
		.and_then(|w| w.checked_add(frac))
		.ok_or_else(|| "Amount is too large.".to_owned())
}

fn group_thousands(n: u64) -> String {
	let s = n.to_string();
	let mut out = String::with_capacity(s.len() + s.len() / 3);
	for (i, ch) in s.chars().enumerate() {
		if i > 0 && (s.len() - i).is_multiple_of(3) {
			out.push(',');
		}
		out.push(ch);
	}
	out
}

#[cfg(test)]
mod tests {
	use super::{format_qty, parse_qty};

	#[test]
	fn scale_zero_is_integer() {
		assert_eq!(format_qty(1234, 0), "1,234");
	}

	#[test]
	fn scale_two_pads_fraction() {
		assert_eq!(format_qty(123_450, 2), "1,234.50");
		assert_eq!(format_qty(5, 2), "0.05");
		assert_eq!(format_qty(0, 2), "0.00");
	}

	#[test]
	fn large_whole() {
		assert_eq!(format_qty(1_000_000_000, 0), "1,000,000,000");
	}

	#[test]
	fn parse_scale_zero() {
		assert_eq!(parse_qty("1,234", 0).unwrap(), 1234);
		assert_eq!(parse_qty("100", 0).unwrap(), 100);
		assert!(parse_qty("1.0", 0).is_err());
		assert!(parse_qty("", 0).is_err());
	}

	#[test]
	fn parse_scale_two() {
		assert_eq!(parse_qty("100", 2).unwrap(), 10_000);
		assert_eq!(parse_qty("100.5", 2).unwrap(), 10_050);
		assert_eq!(parse_qty("1,234.50", 2).unwrap(), 123_450);
		assert_eq!(parse_qty(".5", 2).unwrap(), 50);
		assert!(parse_qty("1.234", 2).is_err());
		assert!(parse_qty("-1", 2).is_err());
	}
}
