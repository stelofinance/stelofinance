//! Display formatting for STDB quantities (asset scale → human decimal)
//! and transfer timestamps (relative time + UTC day headings).

use std::time::{SystemTime, UNIX_EPOCH};

/// Unix microseconds now (0 if the clock is before the epoch).
pub fn unix_now_micros() -> i64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_micros() as i64)
		.unwrap_or(0)
}

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

const MONTHS: [&str; 12] = [
	"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Relative time from `then_micros` to `now_micros` (both Unix µs).
///
/// Buckets: just now → minutes → hours → Yesterday → N days → `12 Aug` (`2025` if another year).
pub fn format_rel_time(then_micros: i64, now_micros: i64) -> String {
	let then = then_micros.div_euclid(1_000_000);
	let now = now_micros.div_euclid(1_000_000);
	let delta = now.saturating_sub(then);
	if delta < 45 {
		return "just now".into();
	}
	if delta < 90 {
		return "1 minute ago".into();
	}
	if delta < 3_600 {
		return format!("{} minutes ago", delta / 60);
	}
	if delta < 5_400 {
		return "1 hour ago".into();
	}
	if delta < 86_400 {
		return format!("{} hours ago", delta / 3_600);
	}
	if delta < 172_800 {
		return "Yesterday".into();
	}
	if delta < 7 * 86_400 {
		return format!("{} days ago", delta / 86_400);
	}
	format_civil_day(then, now)
}

/// Day-group heading: Today / Yesterday / `12 Aug` (year if not this year). UTC days.
pub fn day_heading(then_micros: i64, now_micros: i64) -> String {
	let then_day = then_micros.div_euclid(1_000_000).div_euclid(86_400);
	let now_day = now_micros.div_euclid(1_000_000).div_euclid(86_400);
	match now_day.saturating_sub(then_day) {
		0 => "Today".into(),
		1 => "Yesterday".into(),
		_ => format_civil_day(
			then_micros.div_euclid(1_000_000),
			now_micros.div_euclid(1_000_000),
		),
	}
}

fn format_civil_day(then_secs: i64, now_secs: i64) -> String {
	let (y, m, d) = civil_from_unix_days(then_secs.div_euclid(86_400));
	let (ny, _, _) = civil_from_unix_days(now_secs.div_euclid(86_400));
	let month = MONTHS[usize::from(m.saturating_sub(1)).min(11)];
	if y == ny {
		format!("{d} {month}")
	} else {
		format!("{d} {month} {y}")
	}
}

/// Unix epoch day 0 = 1970-01-01. Howard Hinnant's civil-from-days.
pub fn civil_from_unix_days(unix_days: i64) -> (i32, u8, u8) {
	let z = unix_days + 719_468;
	let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
	let doe = (z - era * 146_097) as u64;
	let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
	let y = yoe as i64 + era * 400;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let d = doy - (153 * mp + 2) / 5 + 1;
	let m = if mp < 10 { mp + 3 } else { mp - 9 };
	let y = if m <= 2 { y + 1 } else { y };
	(y as i32, m as u8, d as u8)
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
	use super::{civil_from_unix_days, day_heading, format_qty, format_rel_time, parse_qty};

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

	#[test]
	fn unix_epoch_day_is_1970_01_01() {
		assert_eq!(civil_from_unix_days(0), (1970, 1, 1));
		// 2000-01-01 = 946684800s = 10957 days
		assert_eq!(civil_from_unix_days(10_957), (2000, 1, 1));
	}

	#[test]
	fn rel_time_buckets() {
		let now = 1_000_000i64;
		assert_eq!(format_rel_time(now, now), "just now");
		assert_eq!(format_rel_time(now - 120 * 1_000_000, now), "2 minutes ago");
		assert_eq!(
			format_rel_time(now - 3 * 3600 * 1_000_000, now),
			"3 hours ago"
		);
		assert_eq!(
			format_rel_time(now - 2 * 86_400 * 1_000_000, now),
			"2 days ago"
		);
	}

	#[test]
	fn day_headings() {
		let today = 20 * 86_400 * 1_000_000;
		assert_eq!(day_heading(today, today), "Today");
		assert_eq!(day_heading(today - 86_400 * 1_000_000, today), "Yesterday");
		assert_eq!(
			day_heading(today - 10 * 86_400 * 1_000_000, today),
			"11 Jan"
		);
	}
}
