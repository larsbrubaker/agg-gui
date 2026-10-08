//! Where the word keys move the caret: a port of agg-sharp's
//! `Gui/TextWidgets/TextCaretNavigation.cs` (`IndexOfNextToken`,
//! `IndexOfPreviousToken`), which `InternalTextEditWidget` exposes as its
//! public statics of the same names.
//!
//! Control+Right (Option+Right on a Mac) stops at the start of the next
//! token, past the spaces after a word; Control+Left stops at the start of
//! the previous one. A run of one repeated break character (`+++`, `//`) is
//! a token of its own, and a newline is a stop. Indices here are character
//! indices, as C#'s are (C# strings index UTF-16 code units, which agree
//! with characters for everything but astral-plane text);
//! `text_field_core::next_word_boundary`/`prev_word_boundary` wrap these in
//! the byte offsets `TextField` and `TextArea` keep.

/// C#'s `WordBreakChars`: the characters that end a word.
fn is_word_break(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t' // white space characters
            | '\'' | '"' | '`' // quotes
            | ',' | '.' | '?' | '!' | '@' | '&' // punctuation
            | '(' | ')' | '<' | '>' | '[' | ']' | '{' | '}' // parents (or equivalent)
            | '-' | '+' | '*' | '/' | '=' | '\\' | '#' | '$' | '^' | '|' | '°' | '²' | '³' // math symbols
    )
}

/// C#'s `WordBreakCharsAndCR`.
fn is_word_break_or_new_line(c: char) -> bool {
    c == '\n' || is_word_break(c)
}

fn is_space_or_tab(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Where Control+Right (Option+Right on Mac) moves the caret to from
/// `cursor` (a character index).
pub fn index_of_next_token(text: &str, cursor: usize) -> usize {
    let text: Vec<char> = text.chars().collect();
    let length = text.len();
    let mut insert = cursor;
    if insert >= length {
        // If we are already at the end, return.
        return length;
    }

    // if we are starting an a CR
    if text[insert] == '\n' {
        // If we are on a CR advance one (goto next line)
        insert += 1;
        // and skip ' ' and '\t'
        while insert < length && is_space_or_tab(text[insert]) {
            insert += 1;
        }
        return insert;
    } else if is_word_break(text[insert]) {
        // we are starting on a work break char
        // while we are on the same char advance
        let current = text[insert];
        while insert < length && text[insert] == current {
            insert += 1;
        }
    } else {
        // we are starting on a normal character
        while insert < length && !is_word_break_or_new_line(text[insert]) {
            insert += 1;
        }
        // and also skip ' ' and '\t'
        while insert < length && is_space_or_tab(text[insert]) {
            insert += 1;
        }
    }
    insert
}

/// Where Control+Left (Option+Left on Mac) moves the caret to from
/// `cursor` (a character index).
pub fn index_of_previous_token(text: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    let text: Vec<char> = text.chars().collect();
    if text.is_empty() {
        return 0;
    }
    // C# works in ints and walks to -1; `prev` here is that index plus one,
    // so `text[prev - 1]` is C#'s `text[prevToken]`.
    let mut prev = cursor.min(text.len()); // = Math.Min(text.Length - 1, cursor - 1) + 1
    let token = text[prev - 1];

    if token == '\n' {
        if prev - 1 > 0 && text[prev - 2] == '\n' {
            return prev - 1;
        }
        prev -= 1;
    } else if is_space_or_tab(token) {
        // the token to the left is a breaking character
        prev -= 1;
        while prev > 0 && is_space_or_tab(text[prev - 1]) {
            // skip back the entire token
            prev -= 1;
        }
    } else if is_word_break(token) {
        // the token to the left is a breaking character
        prev -= 1;
        while prev > 0 && text[prev - 1] == token {
            // skip back the entire token
            prev -= 1;
        }
        return prev;
    }

    // the token to the left is normal character skip until a break
    while prev > 0 && !is_word_break_or_new_line(text[prev - 1]) {
        // skip back until we are on a word break
        prev -= 1;
    }
    prev
}
