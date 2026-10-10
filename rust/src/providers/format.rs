//! Display formatting shared by ports of upstream plugin providers.
//!
//! These mirror the helpers in the upstream provider plugin prelude
//! (`ctx.format.number`, `ctx.format.usd`) and JavaScript's
//! `Number.prototype.toFixed`, so ported rows render the same strings, plus
//! the small dollar and count formats several native providers share.

/// `ctx.format.number(value, { maximumFractionDigits })`: fixed to
/// `max_fraction_digits`, trailing fractional zeros trimmed, and the integer
/// part grouped with commas.
pub(crate) fn number(value: f64, max_fraction_digits: usize) -> String {
    format_number(value, 0, max_fraction_digits)
}

/// `ctx.format.usd(value)`: a dollar amount with exactly two fractional
/// digits, grouped, and the sign before the dollar sign.
pub(crate) fn usd(value: f64) -> String {
    let sign = if value < 0.0 { "-$" } else { "$" };
    format!("{sign}{}", format_number(value.abs(), 2, 2))
}

/// A dollar amount with two fractional digits and no grouping, as
/// `format!("${value:.2}")` prints it (`$1234.50`).
pub(crate) fn usd_plain(value: f64) -> String {
    format!("${value:.2}")
}

/// `$95.50` / `-$1.25` with no grouping; a negative that rounds to zero
/// shows no sign.
pub(crate) fn usd_signed(value: f64) -> String {
    let magnitude = format!("{:.2}", value.abs());
    if value < 0.0 && magnitude != "0.00" {
        format!("-${magnitude}")
    } else {
        format!("${magnitude}")
    }
}

/// An integer count with comma digit groups (`1,234,567`).
pub(crate) fn count(value: u64) -> String {
    group_thousands(&value.to_string())
}

/// Whole values without decimals, anything else with two (`6182`, `57.20`).
pub(crate) fn whole_or_two_decimals(value: f64) -> String {
    if (value - value.round()).abs() < f64::EPSILON {
        format!("{:.0}", value)
    } else {
        format!("{:.2}", value)
    }
}

/// JavaScript `Number.prototype.toFixed(digits)`.
///
/// Rust's formatter rounds an exact binary tie to even; JavaScript picks the
/// larger magnitude. Magnitudes of 1e21 or more use JavaScript's exponent
/// notation, as `toFixed` does.
pub(crate) fn to_fixed(value: f64, digits: usize) -> String {
    if !value.is_finite() {
        return js_string(value);
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let magnitude = value.abs();
    if magnitude >= 1e21 {
        return format!("{sign}{}", js_string(magnitude));
    }
    // An exact tie is a dyadic fraction whose expansion ends at digit
    // `digits + 1`. Any other double that can round at `digits` lies at least
    // 1e-16 of its own magnitude away from the tie, so forty extra digits
    // never print a near-tie as "5000...".
    let probe = format!("{:.*}", digits + 40, magnitude);
    let tail = probe
        .split_once('.')
        .and_then(|(_, fraction)| fraction.get(digits..))
        .unwrap_or("");
    let tie = tail.starts_with('5') && tail[1..].bytes().all(|byte| byte == b'0');
    let magnitude = if tie { magnitude.next_up() } else { magnitude };
    format!("{sign}{magnitude:.digits$}")
}

/// The prelude's `formatNumber`: `toFixed(max)` of the magnitude, trailing
/// zeros trimmed down to `min` fractional digits, digit groups of three.
fn format_number(value: f64, min_fraction_digits: usize, max_fraction_digits: usize) -> String {
    if !value.is_finite() {
        return js_string(value);
    }
    let fixed = to_fixed(value.abs(), max_fraction_digits);
    let (integer, fraction) = fixed.split_once('.').unwrap_or((fixed.as_str(), ""));
    let mut fraction = fraction;
    while fraction.len() > min_fraction_digits && fraction.ends_with('0') {
        fraction = &fraction[..fraction.len() - 1];
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let integer = group_thousands(integer);
    if fraction.is_empty() {
        format!("{sign}{integer}")
    } else {
        format!("{sign}{integer}.{fraction}")
    }
}

/// Groups a run of ASCII digits with commas. Anything else (JavaScript's
/// exponent form) is returned unchanged, as the prelude's regex does.
fn group_thousands(integer: &str) -> String {
    if integer.is_empty() || !integer.bytes().all(|byte| byte.is_ascii_digit()) {
        return integer.to_string();
    }
    let mut grouped = String::with_capacity(integer.len() + integer.len() / 3);
    for (index, digit) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// JavaScript `String(number)` for the values `toFixed` passes through:
/// non-finite values and magnitudes of 1e21 or more.
fn js_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    // Both languages print the shortest round-trip digits; JavaScript also
    // signs a positive exponent.
    let exponent = format!("{value:e}");
    match exponent.split_once('e') {
        Some((mantissa, power)) if !power.starts_with('-') => format!("{mantissa}e+{power}"),
        _ => exponent,
    }
}

#[cfg(test)]
mod tests {
    use super::{count, number, to_fixed, usd, usd_plain, usd_signed, whole_or_two_decimals};

    #[test]
    fn to_fixed_rounds_exact_ties_away_from_zero_like_javascript() {
        for (value, digits, expected) in [
            (0.5, 0, "1"),
            (1.5, 0, "2"),
            (2.5, 0, "3"),
            (0.125, 2, "0.13"),
            (0.375, 2, "0.38"),
            (-2.5, 0, "-3"),
            (1.005, 2, "1.00"),
            (1.0049999, 2, "1.00"),
            (12.5, 0, "13"),
            (9.95, 1, "9.9"),
            (0.0, 2, "0.00"),
            (-0.0, 1, "0.0"),
            (123.456, 0, "123"),
        ] {
            assert_eq!(to_fixed(value, digits), expected, "{value} to {digits}");
        }
    }

    #[test]
    fn to_fixed_switches_to_exponent_notation_at_1e21() {
        assert_eq!(to_fixed(1e21, 2), "1e+21");
        assert_eq!(to_fixed(1.5e25, 0), "1.5e+25");
        assert_eq!(to_fixed(-1e21, 0), "-1e+21");
        assert_eq!(
            to_fixed(999_999_999_999_999_900_000.0, 0),
            "999999999999999868928"
        );
        assert_eq!(to_fixed(f64::NAN, 2), "NaN");
        assert_eq!(to_fixed(f64::INFINITY, 2), "Infinity");
    }

    #[test]
    fn number_trims_zeros_and_groups_thousands_like_the_plugin_prelude() {
        for (value, digits, expected) in [
            (0.0, 2, "0"),
            (12.0, 2, "12"),
            (18.5, 2, "18.5"),
            (42.5, 2, "42.5"),
            (12_500.0, 2, "12,500"),
            (400_000.0, 2, "400,000"),
            (1_234_567.891, 2, "1,234,567.89"),
            (12_500.125, 2, "12,500.13"),
            (999.999, 2, "1,000"),
            (100.0, 0, "100"),
            (1_000.0, 0, "1,000"),
            (-1_234.5, 2, "-1,234.5"),
            (-0.001, 2, "-0"),
            (0.1 + 0.2, 2, "0.3"),
        ] {
            assert_eq!(number(value, digits), expected, "{value} to {digits}");
        }
        assert_eq!(number(f64::NAN, 2), "NaN");
        assert_eq!(number(f64::NEG_INFINITY, 2), "-Infinity");
        assert_eq!(number(1e21, 2), "1e+21");
    }

    #[test]
    fn usd_keeps_two_digits_groups_and_signs_before_the_dollar() {
        assert_eq!(usd(0.0), "$0.00");
        assert_eq!(usd(1_234.5), "$1,234.50");
        assert_eq!(usd(12.345), "$12.35");
        assert_eq!(usd(-1.0), "-$1.00");
        assert_eq!(usd(1_000_000.0), "$1,000,000.00");
    }

    #[test]
    fn usd_plain_and_signed_keep_two_digits_without_grouping() {
        assert_eq!(usd_plain(1_234.5), "$1234.50");
        assert_eq!(usd_plain(-1.0), "$-1.00");
        assert_eq!(usd_signed(1_234.5), "$1234.50");
        assert_eq!(usd_signed(-1.25), "-$1.25");
        assert_eq!(usd_signed(-0.001), "$0.00");
    }

    #[test]
    fn count_groups_digits_by_three() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1_000), "1,000");
        assert_eq!(count(1_234_567), "1,234,567");
        assert_eq!(count(u64::MAX), "18,446,744,073,709,551,615");
    }

    #[test]
    fn whole_or_two_decimals_drops_zero_cents_only() {
        assert_eq!(whole_or_two_decimals(6182.0), "6182");
        assert_eq!(whole_or_two_decimals(57.2), "57.20");
    }
}
