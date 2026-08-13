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
	use super::format_qty;

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
}
