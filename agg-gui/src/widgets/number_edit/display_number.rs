//! A number as the user sees and types it — port of agg-sharp
//! `agg/Helpers/DisplayNumber.cs`, the one place that decides both for
//! [`NumberEdit`](super::NumberEdit).
//!
//! agg-gui has no culture layer, so the number part of C#'s
//! `CultureInfo.CurrentCulture` is passed in as a [`NumberFormat`] (its
//! decimal and group separators); the default is the invariant culture.
//! Shown text is C#'s `"R"` / `"0.###"` in that culture: the invariant text
//! with its `.` made the format's decimal separator, never grouped. Typed
//! text is read invariant first (`"1.5"` is 1.5 for everyone), then in the
//! format (`"1,5"` is 1.5 where `,` is the decimal), neither read allowing
//! group separators; a thousands-grouped whole number in the format
//! (`"1.000"` with a `.` group, `"1,000"` with a `,` one) is refused rather
//! than guessed.

/// The number part of a C# `CultureInfo` that [`NumberEdit`](super::NumberEdit)
/// shows and reads numbers in: C#'s `NumberFormat.NumberDecimalSeparator`
/// and `NumberGroupSeparator` (`None` where the culture has none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberFormat {
    pub decimal_separator: char,
    pub group_separator: Option<char>,
}

impl NumberFormat {
    /// A format with these separators.
    pub const fn new(decimal_separator: char, group_separator: Option<char>) -> Self {
        Self {
            decimal_separator,
            group_separator,
        }
    }

    /// C# `CultureInfo.InvariantCulture`'s numbers: `.` decimal, `,` groups.
    pub const fn invariant() -> Self {
        Self::new('.', Some(','))
    }
}

impl Default for NumberFormat {
    fn default() -> Self {
        Self::invariant()
    }
}

/// C# `DisplayNumber.Format(value, format)` under the invariant culture; see
/// [`format_in`].
pub fn format(value: f64, max_decimal_places: i32) -> String {
    format_invariant(value, max_decimal_places)
}

/// C# `DisplayNumber.Format(value, format)` in `number_format`: the shortest
/// round-trip text (`"R"`) when `max_decimal_places <= 0`, otherwise C#'s
/// custom `"0.###…"` pattern with `min(10, max_decimal_places)` optional
/// decimals, written with the format's decimal separator and never grouped.
pub fn format_in(value: f64, max_decimal_places: i32, number_format: &NumberFormat) -> String {
    let text = format_invariant(value, max_decimal_places);
    // the invariant text has no group separators, so its only '.' is the decimal point
    if number_format.decimal_separator == '.' {
        text
    } else {
        text.replace('.', &number_format.decimal_separator.to_string())
    }
}

fn format_invariant(value: f64, max_decimal_places: i32) -> String {
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

/// C# `DisplayNumber.TryRead(text, out value)` under the invariant culture;
/// see [`try_read_in`].
pub fn try_read(text: &str) -> Option<f64> {
    try_read_in(text, &NumberFormat::invariant())
}

/// C# `DisplayNumber.TryRead(text, culture, out value)`: text that
/// [`is_ambiguous_grouping_in`] the format is refused; otherwise it is read
/// invariant first, then with the format's decimal separator
/// (`double.TryParse(text, NumberStyles.Float, culture)`, which takes no
/// group separators and, where the decimal is not a point, no point at all).
pub fn try_read_in(text: &str, number_format: &NumberFormat) -> Option<f64> {
    if is_ambiguous_grouping_in(text, number_format) {
        return None;
    }
    parse_invariant(text).or_else(|| {
        let decimal = number_format.decimal_separator;
        if decimal == '.' || text.contains('.') {
            return None;
        }
        parse_invariant(&text.replace(decimal, "."))
    })
}

/// `double.TryParse(text, NumberStyles.Float, InvariantCulture)` — optional
/// surrounding white space, an optional sign, digits with at most one `.`,
/// an optional exponent; `Infinity` / `NaN` in any case.
fn parse_invariant(text: &str) -> Option<f64> {
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

/// C# `DisplayNumber.IsAmbiguousGrouping(text, culture)`: text that is
/// exactly a thousands-grouped whole number in the format — one to three
/// digits not starting with 0, then one or more groups of its group
/// separator and exactly three digits (`"1,000"`, `"12,500,000"` invariant;
/// `"1.000"` with a `.` group), optionally signed, surrounding white space
/// ignored (C# `Trim()`). Never true for a format without a group separator
/// or with the same one as its decimal.
pub fn is_ambiguous_grouping_in(text: &str, number_format: &NumberFormat) -> bool {
    let Some(group) = number_format.group_separator else {
        return false;
    };
    if group == number_format.decimal_separator {
        return false;
    }
    let text = text.trim();
    let text = text
        .strip_prefix('+')
        .or_else(|| text.strip_prefix('-'))
        .unwrap_or(text);
    let mut groups = text.split(group);
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
            assert!(
                is_ambiguous_grouping_in(grouped, &NumberFormat::invariant()),
                "{grouped:?}"
            );
        }
        for plain in ["1,5", "1,0000", "0,500", "1000", "1.000", ",000", "1,000.5"] {
            assert!(
                !is_ambiguous_grouping_in(plain, &NumberFormat::invariant()),
                "{plain:?}"
            );
        }
    }
}
