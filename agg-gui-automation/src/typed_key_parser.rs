//! `TypedKeyParser` — the port of agg-sharp `GuiAutomation/TypedKeyParser.cs`.
//!
//! Turns the strings tests hand `type_text` into key strokes.  The spelling is
//! a deliberately small subset of what Windows' SendKeys accepts: `^` is the
//! command modifier on the key that follows it, `^+` is command and shift, and
//! a name in braces — `{Enter}`, `{Esc}`, `{BACKSPACE}` — is that key rather
//! than those letters.  A bare `+` is *not* shift, unlike SendKeys: tests type
//! expressions like `=40 + 5` into fields, and reading that plus as a modifier
//! would quietly turn the rest of the string into something else.
//!
//! An unrecognised brace token is an error rather than being typed out one
//! character at a time.  That silence is the bug this parser was extracted
//! to fix: a test that pressed nothing it meant to press still went green,
//! because the keys it did send were harmless.
//!
//! Strokes carry C#'s [`Keys`] values (the tests check them 1:1);
//! `key_mapping.rs` turns them into the `agg_gui` key events the `App` takes,
//! where `^` becomes the platform's command modifier (Cmd on macOS).

use std::fmt;

use crate::keys::Keys;

/// One key press the automation runner is going to put through a widget tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypedKey {
    key: Keys,
    character: char,
}

impl TypedKey {
    /// A stroke: `key` with any modifier bits already or'd in, and the
    /// character it types, or `'\0'` when it types nothing.
    pub fn new(key: Keys, character: char) -> Self {
        Self { key, character }
    }

    /// The key code with its modifier bits — what a key event is built from.
    pub fn key(&self) -> Keys {
        self.key
    }

    /// The character this stroke types, or `'\0'` for a stroke that types
    /// nothing.
    pub fn character(&self) -> char {
        self.character
    }

    /// True when the stroke types a character (C# `HasCharacter`: a
    /// KeyPress follows the KeyDown).
    pub fn has_character(&self) -> bool {
        self.character != '\0'
    }
}

/// A type string the parser cannot read (C# throws `ArgumentException`
/// with this message, parameter `textToType`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    message: String,
}

impl ParseError {
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ParseError {}

/// Reads type strings into strokes (C# static class `TypedKeyParser`).
pub struct TypedKeyParser;

/// Characters whose key code is not just their upper-case value — the key
/// each one sits on in a US layout.  Everything not listed here is typed as
/// `(Keys)char.ToUpper(c)`, which is right for letters, digits and space.
///
/// This table is not cosmetic.  ASCII 33-47 share their values with
/// navigation keys — `(Keys)'!'` is PageUp, `'$'` is Home, `'('` is Down,
/// `'.'` is Delete — so typing them by value sent a key a text field acts
/// on, and the field then suppressed the KeyPress that carried the
/// character.  A shifted character stays on its base key without a Shift
/// modifier: the character is what a field types, and a Shift bit would make
/// Shift+key handlers see a chord.
const CHAR_TO_KEYS: &[(char, Keys)] = &[
    ('!', Keys::D1),
    ('@', Keys::D2),
    ('#', Keys::D3),
    ('$', Keys::D4),
    ('%', Keys::D5),
    ('^', Keys::D6),
    ('&', Keys::D7),
    ('*', Keys::D8),
    ('(', Keys::D9),
    (')', Keys::D0),
    ('-', Keys::OEM_MINUS),
    ('_', Keys::OEM_MINUS),
    ('=', Keys::OEMPLUS),
    ('+', Keys::OEMPLUS),
    ('[', Keys::OEM_OPEN_BRACKETS),
    ('{', Keys::OEM_OPEN_BRACKETS),
    (']', Keys::OEM_CLOSE_BRACKETS),
    ('}', Keys::OEM_CLOSE_BRACKETS),
    ('\\', Keys::OEM_PIPE),
    ('|', Keys::OEM_PIPE),
    (';', Keys::OEM_SEMICOLON),
    (':', Keys::OEM_SEMICOLON),
    ('\'', Keys::OEM_QUOTES),
    ('"', Keys::OEM_QUOTES),
    (',', Keys::OEMCOMMA),
    ('<', Keys::OEMCOMMA),
    ('.', Keys::OEM_PERIOD),
    ('>', Keys::OEM_PERIOD),
    ('/', Keys::OEM_QUESTION),
    ('?', Keys::OEM_QUESTION),
    ('`', Keys::OEMTILDE),
    ('~', Keys::OEMTILDE),
];

/// Brace-token spellings that are not the `Keys` member's own name
/// (matched ignoring ASCII case, as C#'s `OrdinalIgnoreCase`).
const TOKEN_ALIASES: &[(&str, Keys)] = &[
    ("ESC", Keys::ESCAPE),
    ("BACKSPACE", Keys::BACK),
    ("BKSP", Keys::BACK),
    ("BS", Keys::BACK),
    ("DEL", Keys::DELETE),
    ("PGUP", Keys::PAGE_UP),
    ("PGDN", Keys::PAGE_DOWN),
    ("ENTER", Keys::ENTER),
    ("BREAK", Keys::CANCEL),
];

impl TypedKeyParser {
    /// Read a type string into the strokes it stands for, in order.
    ///
    /// Errors when a brace token is unterminated or names no key.
    pub fn parse(text_to_type: &str) -> Result<Vec<TypedKey>, ParseError> {
        let mut strokes = Vec::new();
        // C# indexes UTF-16 code units; tests type BMP text, so chars match.
        let chars: Vec<char> = text_to_type.chars().collect();

        let mut index = 0;
        while index < chars.len() {
            let mut modifiers = Keys::NONE;

            // Modifiers bind to the single stroke that follows them, so they
            // are read here rather than carried across the loop.
            if chars[index] == '^' && index + 1 < chars.len() {
                modifiers |= Keys::CONTROL;
                index += 1;

                if chars[index] == '+' && index + 1 < chars.len() {
                    modifiers |= Keys::SHIFT;
                    index += 1;
                }
            }

            if chars[index] == '{' {
                let Some(close) = (index..chars.len()).find(|&i| chars[i] == '}') else {
                    return Err(ParseError {
                        message: format!(
                            "'{text_to_type}' opens a key token with '{{' that is never closed with '}}'."
                        ),
                    });
                };

                let token: String = chars[index + 1..close].iter().collect();
                strokes.push(TypedKey::new(
                    modifiers | parse_token(&token, text_to_type)?,
                    '\0',
                ));
                index = close + 1;
                continue;
            }

            let character = chars[index];
            let key = CHAR_TO_KEYS
                .iter()
                .find(|(c, _)| *c == character)
                .map(|(_, k)| *k)
                .unwrap_or_else(|| Keys(char_to_upper(character) as i32));

            // A control chord types no character: the KeyPress that would
            // follow it on a real keyboard carries a control code, and every
            // widget that acts on the chord acts on the KeyDown.
            let types_a_character = !modifiers.contains(Keys::CONTROL);

            strokes.push(TypedKey::new(
                modifiers | key,
                if types_a_character { character } else { '\0' },
            ));
            index += 1;
        }

        Ok(strokes)
    }
}

/// C# `char.ToUpper`: the single-character upper-case mapping, or the
/// character itself when its upper case is not one character.
fn char_to_upper(c: char) -> char {
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

fn parse_token(token: &str, text_to_type: &str) -> Result<Keys, ParseError> {
    if let Some((_, alias)) = TOKEN_ALIASES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(token))
    {
        return Ok(*alias);
    }

    if let Some(parsed) = Keys::try_parse(token) {
        return Ok(parsed);
    }

    // "{Ctrl+a}" reads naturally but is not a spelling SendKeys knows — the
    // Windows input method would type it out literally — so point at the one
    // both input methods share.
    let token_chars: Vec<char> = token.chars().collect();
    if let Some(plus) = token_chars.iter().position(|&c| c == '+') {
        if plus > 0 && plus < token_chars.len() - 1 {
            return Err(ParseError {
                message: format!(
                    "'{{{token}}}' in '{text_to_type}' puts a modifier inside braces. Write control as ^ before \
                     the key instead: \"^a\" for Ctrl+A (select all), \"^+z\" for Ctrl+Shift+Z, \"^{{Home}}\" \
                     for Ctrl+Home."
                ),
            });
        }
    }

    Err(ParseError {
        message: format!(
            "'{{{token}}}' in '{text_to_type}' does not name a key. Use a Keys member name \
             ({{Enter}}, {{Escape}}, {{Left}}, {{F4}}) or one of the SendKeys spellings ({{ESC}}, {{BACKSPACE}}, \
             {{DEL}}, {{PGUP}}, {{PGDN}})."
        ),
    })
}
