//! The number state behind a [`NumberEdit`](super::NumberEdit): its options,
//! range, parser and last good value, and the C# `InternalNumberEdit` logic
//! that reads only them — what a keystroke may type (`CanType`), how text
//! reads as a value (the `Value` getter), the range clamp (`ValidateRange`)
//! and whether a commit has anything readable to keep (`OnEditComplete`'s
//! guard).
//!
//! It is shared (`Rc<RefCell<_>>`) between the widget and the whole-edit
//! judge it installs on its [`TextField`](crate::widgets::TextField)
//! ([`with_text_filter`](crate::widgets::TextField::with_text_filter)), so
//! the judge sees the parser and options of the moment.

use super::display_number::{self, NumberFormat};

/// C# `TextValueParser`: reads text a plain number parse cannot (a unit
/// written on the number, say); `None` means "not mine".
pub type TextValueParser = Box<dyn Fn(&str) -> Option<f64>>;

pub(super) struct NumberState {
    pub(super) allow_negatives: bool,
    pub(super) allow_decimals: bool,
    pub(super) min_value: f64,
    pub(super) max_value: f64,
    pub(super) increment: f64,
    /// C# `MaxDecimalsPlaces`; `<= 0` shows the shortest round-trip text.
    pub(super) max_decimal_places: i32,
    pub(super) allow_expression_entry: bool,
    pub(super) parser: Option<TextValueParser>,
    pub(super) last_parsed_text: Option<String>,
    /// The last value set or committed; an unreadable entry falls back to it.
    pub(super) last_value: f64,
    /// The separators numbers are shown and read with (C#'s
    /// `CultureInfo.CurrentCulture`).
    pub(super) number_format: NumberFormat,
}

impl NumberState {
    /// C#'s `allowedChars`: the digits, `-` with negatives, and with
    /// decimals both `.` (read invariant everywhere) and the format's own
    /// decimal separator (a comma-decimal keyboard writes `,`). A group
    /// separator is never typeable.
    fn is_allowed_char(&self, c: char) -> bool {
        c.is_ascii_digit()
            || (c == '-' && self.allow_negatives)
            || ((c == '.' || c == self.number_format.decimal_separator) && self.allow_decimals)
    }

    /// C# `CanType`, for an insertion of `inserted` that would leave `typed`
    /// in the field. C# judges one typed character; a paste is judged the
    /// same way with every character it inserts held to the allowed set.
    pub(super) fn can_type(&self, inserted: &str, typed: &str) -> bool {
        // An entry the owner will take over as an expression is judged whole
        // rather than character by character - nothing in "=10*2" is a
        // number, and it is not this field's to read anyway.
        if self.allow_expression_entry && typed.starts_with('=') {
            return true;
        }

        // letters and spaces are only typeable because a parser is there to read them
        let parser = self.parser.is_some();
        if !inserted
            .chars()
            .all(|c| self.is_allowed_char(c) || (parser && (c.is_alphabetic() || c == ' ')))
        {
            return false;
        }

        // the starts of a number, on their way to being one
        let decimal = self.number_format.decimal_separator;
        let mut chars = typed.chars();
        let lone_decimal = |c: Option<char>| c == Some('.') || c == Some(decimal);
        let (first, second, rest) = (chars.next(), chars.next(), chars.next());
        if lone_decimal(first) && second.is_none() && self.allow_decimals {
            return true;
        }
        if typed == "-" && self.allow_negatives {
            return true;
        }
        if first == Some('-')
            && lone_decimal(second)
            && rest.is_none()
            && self.allow_decimals
            && self.allow_negatives
        {
            return true;
        }

        // Text on its way to something only the parser can read - "2i" before
        // "2in" - is not a number and is not parsable yet either, so there is
        // nothing to check per keystroke. With a parser in play the commit is
        // where an unreadable entry gets sorted out.
        if parser && typed.chars().any(char::is_alphabetic) {
            return true;
        }

        // "1.000" under a '.' group is refused when committed, but has to be
        // typeable on the way to "1.0005"
        let format = &self.number_format;
        display_number::try_read_in(typed, format).is_some()
            || display_number::is_ambiguous_grouping_in(typed, format)
    }

    /// C#'s `Value` getter: the parser's reading, else the plain numeric one,
    /// else the error value. Every read re-decides where the value came from,
    /// so `last_parsed_text` can never go stale.
    pub(super) fn read_value(&mut self, text: &str) -> f64 {
        self.last_parsed_text = None;
        if let Some(parsed) = self.parser.as_ref().and_then(|parse| parse(text)) {
            self.last_parsed_text = Some(text.to_string());
            return parsed;
        }
        let error_return = self.min_value.max(self.max_value.min(0.0));
        if matches!(text, "" | "." | "-" | "-.") {
            return error_return;
        }
        display_number::try_read_in(text, &self.number_format).unwrap_or(error_return)
    }

    /// C# `ValidateRange`.
    pub(super) fn validate_range(&self, value: f64) -> f64 {
        let mut value = value;
        if value < self.min_value {
            value = self.min_value;
        }
        if value > self.max_value {
            value = self.max_value;
        }
        value
    }

    /// The guard at the top of C#'s `OnEditComplete`: whether `text` holds
    /// something to commit — an expression the owner reads (with expression
    /// entry on), the parser's spelling, or a plain number. Otherwise the
    /// field falls back to its last good value.
    pub(super) fn commit_is_readable(&self, text: &str) -> bool {
        (self.allow_expression_entry && text.trim_start().starts_with('='))
            || self.parser.as_ref().and_then(|parse| parse(text)).is_some()
            || self.reads_as_number(text)
    }

    /// C# `ReadsAsNumber`: `text` read in the box's number format.
    pub(super) fn reads_as_number(&self, text: &str) -> bool {
        display_number::try_read_in(text, &self.number_format).is_some()
    }

    /// C# `DisplayNumber.Format(value, format)` with the box's decimal places
    /// and number format.
    pub(super) fn format_value(&self, value: f64) -> String {
        display_number::format_in(value, self.max_decimal_places, &self.number_format)
    }

    /// C#'s `MinValue` / `MaxValue` setter checks.
    pub(super) fn check_limit(&self, value: f64) -> Result<(), NumberEditError> {
        if !self.allow_negatives && value < 0.0 {
            return Err(NumberEditError::NegativeNotAllowed);
        }
        if !self.allow_decimals && value != value.trunc() {
            return Err(NumberEditError::FractionNotAllowed);
        }
        Ok(())
    }
}

/// C#'s `InternalNumberEdit` constructor / limit-setter exceptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberEditError {
    /// A negative start, min, max or increment without `allow_negatives`.
    NegativeNotAllowed,
    /// A fractional start, min, max or increment without `allow_decimals`.
    FractionNotAllowed,
}

impl std::fmt::Display for NumberEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // C#'s constructor messages.
        f.write_str(match self {
            Self::NegativeNotAllowed => {
                "To have a startingValue, min, max, or increment be negative, you need to set 'allowNegatives' to true."
            }
            Self::FractionNotAllowed => {
                "To have a fractional startingValue, min, max, or increment you need to set 'allowDecimals' to true."
            }
        })
    }
}

impl std::error::Error for NumberEditError {}
