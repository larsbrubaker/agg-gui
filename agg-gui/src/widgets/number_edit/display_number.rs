//! A number as the user sees and types it — port of agg-sharp
//! `agg/Helpers/DisplayNumber.cs`, the one place that decides both for
//! [`NumberEdit`](super::NumberEdit).
//!
//! agg-gui has no per-user number culture, so this is C#'s `DisplayNumber`
//! under the invariant culture: shown with a `.` decimal and never grouped,
//! read with `.` as the decimal, and a thousands-grouped whole number
//! (`"1,000"`) refused rather than guessed — C# reads typed text invariant
//! first, and under the invariant culture its group separator is `,`.

/// C# `DisplayNumber.Format(value, format)`: the shortest round-trip text
/// (`"R"`) when `max_decimal_places <= 0`, otherwise C#'s custom
/// `"0.###…"` pattern with `min(10, max_decimal_places)` optional decimals.
pub fn format(value: f64, max_decimal_places: i32) -> String {
    if max_decimal_places > 0 && value.is_finite() {
        let places = max_decimal_places.min(10) as usize;
        let mut text = format!("{value:.places$}");
        if text.contains('.') {
            while text.ends_with('0') {
                text.pop();
            }
            if text.ends_with('.') {
                text.pop();
            }
        }
        return text;
    }
    format_round_trip(value)
}

/// C#'s `double.ToString("R", invariant)` (.NET Core 3.0 and later): the
/// shortest digits that read back to `value`, in scientific notation
/// (`"1E+15"`, `"1E-05"`) when the decimal exponent is at least 15 or below
/// -4.
pub fn format_round_trip(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "∞" } else { "-∞" }.to_string();
    }
    if value == 0.0 {
        // C# keeps the sign of negative zero: (-0.0).ToString() is "-0".
        return if value.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    // Rust's `{:e}` is the shortest round-trip digits too; only the layout
    // differs from C#.
    let sci = format!("{:e}", value.abs());
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let sign = if value < 0.0 { "-" } else { "" };
    if !(-4..15).contains(&exponent) {
        let mut text = String::from(sign);
        text.push_str(&digits[..1]);
        if digits.len() > 1 {
            text.push('.');
            text.push_str(&digits[1..]);
        }
        let exp_sign = if exponent < 0 { '-' } else { '+' };
        text.push_str(&format!("E{exp_sign}{:02}", exponent.abs()));
        return text;
    }
    let mut text = String::from(sign);
    if exponent < 0 {
        text.push_str("0.");
        for _ in 0..(-exponent - 1) {
            text.push('0');
        }
        text.push_str(&digits);
    } else {
        let int_len = exponent as usize + 1;
        if digits.len() <= int_len {
            text.push_str(&digits);
            for _ in digits.len()..int_len {
                text.push('0');
            }
        } else {
            text.push_str(&digits[..int_len]);
            text.push('.');
            text.push_str(&digits[int_len..]);
        }
    }
    text
}

/// C# `DisplayNumber.TryRead(text, out value)` under the invariant culture:
/// `double.TryParse(text, NumberStyles.Float, InvariantCulture)` — optional
/// surrounding white space, an optional sign, digits with at most one `.`,
/// an optional exponent; `Infinity` / `NaN` in any case — refusing text that
/// [`is_ambiguous_grouping`].
pub fn try_read(text: &str) -> Option<f64> {
    if is_ambiguous_grouping(text) {
        return None;
    }
    // C#'s NumberStyles white space: U+0009..U+000D and the space.
    let trimmed = text.trim_matches(|c: char| c == ' ' || ('\u{9}'..='\u{d}').contains(&c));
    let (negative, body) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let signed = |v: f64| if negative { -v } else { v };
    if body.eq_ignore_ascii_case("infinity") || body == "∞" {
        return Some(signed(f64::INFINITY));
    }
    if body.eq_ignore_ascii_case("nan") {
        return Some(f64::NAN);
    }
    if !is_float_body(body) {
        return None;
    }
    body.parse::<f64>().ok().map(signed)
}

/// `digits [. digits] [(e|E) [sign] digits]` with at least one mantissa
/// digit — what C#'s `NumberStyles.Float` admits after the sign. Checked
/// first because Rust's own parse also takes `inf`, `nan` and `infinity`
/// spellings C# does not.
fn is_float_body(body: &str) -> bool {
    let bytes = body.as_bytes();
    let mut i = 0;
    let mut mantissa_digits = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
        mantissa_digits += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            mantissa_digits += 1;
        }
    }
    if mantissa_digits == 0 {
        return false;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    i == bytes.len()
}

/// C# `DisplayNumber.IsAmbiguousGrouping(text, InvariantCulture)`: text that
/// is exactly a thousands-grouped whole number — one to three digits not
/// starting with 0, then one or more groups of `,` and exactly three digits
/// (`"1,000"`, `"12,500,000"`), optionally signed, surrounding white space
/// ignored (C# `Trim()`).
pub fn is_ambiguous_grouping(text: &str) -> bool {
    let text = text.trim();
    let text = text
        .strip_prefix('+')
        .or_else(|| text.strip_prefix('-'))
        .unwrap_or(text);
    let mut groups = text.split(',');
    let Some(lead) = groups.next() else {
        return false;
    };
    if lead.is_empty()
        || lead.len() > 3
        || lead.starts_with('0')
        || !lead.bytes().all(|b| b.is_ascii_digit())
    {
        return false;
    }
    let mut group_count = 0;
    for group in groups {
        if group.len() != 3 || !group.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        group_count += 1;
    }
    group_count > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_only_formats_like_csharp_round_trip() {
        assert_eq!(format_round_trip(0.0), "0");
        assert_eq!(format_round_trip(120.0), "120");
        assert_eq!(format_round_trip(12.23), "12.23");
        assert_eq!(format_round_trip(-2.5), "-2.5");
        assert_eq!(format_round_trip(0.0001), "0.0001");
        assert_eq!(format_round_trip(0.00001), "1E-05");
        assert_eq!(format_round_trip(1e14), "100000000000000");
        assert_eq!(format_round_trip(1e15), "1E+15");
        assert_eq!(format_round_trip(1.5e20), "1.5E+20");
        assert_eq!(format_round_trip(2147483647.0), "2147483647");
    }

    #[test]
    fn rust_only_formats_max_decimal_places_like_csharp_custom_pattern() {
        assert_eq!(format(1.23456, 3), "1.235");
        assert_eq!(format(2.0, 3), "2");
        assert_eq!(format(2.5, 3), "2.5");
        assert_eq!(format(1.23456, 0), "1.23456");
        assert_eq!(format(0.1, 20), "0.1");
    }

    #[test]
    fn rust_only_reads_what_csharp_float_style_reads() {
        assert_eq!(try_read("12.23"), Some(12.23));
        assert_eq!(try_read(" -1.5 "), Some(-1.5));
        assert_eq!(try_read("+3"), Some(3.0));
        assert_eq!(try_read("1."), Some(1.0));
        assert_eq!(try_read(".5"), Some(0.5));
        assert_eq!(try_read("1e3"), Some(1000.0));
        assert_eq!(try_read("-Infinity"), Some(f64::NEG_INFINITY));
        for unreadable in [
            "", ".", "-", "-.", "1-2", "1.2.3", "1e", "inf", "2in", "=1", "1,000",
        ] {
            assert_eq!(try_read(unreadable), None, "{unreadable:?}");
        }
    }

    #[test]
    fn rust_only_ambiguous_grouping_is_exactly_whole_thousands() {
        for grouped in ["1,000", "12,500", "-1,000,000", " 999,999 "] {
            assert!(is_ambiguous_grouping(grouped), "{grouped:?}");
        }
        for plain in ["1,5", "1,0000", "0,500", "1000", "1.000", ",000", "1,000.5"] {
            assert!(!is_ambiguous_grouping(plain), "{plain:?}");
        }
    }
}
