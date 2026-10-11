//! How [`super::inline_writer`] writes what sits inside its delimiters:
//! escaped plain text, code spans and link tails.  Ports agg-sharp's
//! `RichMarkdownEscaping.cs`.
//!
//! Runs hold decoded text, so everything here re-encodes it to read back
//! unchanged.  Text is examined per `char` (see the module's offset rule in
//! [`super`]); [`super::char_class`] supplies .NET's character classes.

use super::char_class::{is_letter, is_letter_or_digit};

/// An entity reference CommonMark would decode at the start of `text`
/// (`&(#[0-9]{1,7}|#[xX][0-9a-fA-F]{1,6}|[A-Za-z][A-Za-z0-9]{1,31});`); a
/// literal '&' only needs escaping in front of one.
fn entity_start(text: &[char]) -> bool {
    if text.first() != Some(&'&') {
        return false;
    }

    // A run of `class` characters right after `from`, `min..=max` long and closed by ';'.
    let run_then_semicolon = |from: usize, min: usize, max: usize, class: fn(&char) -> bool| {
        let run = text[from.min(text.len())..]
            .iter()
            .take_while(|c| class(c))
            .count();
        (min..=max).contains(&run) && text.get(from + run) == Some(&';')
    };
    match text.get(1) {
        Some('#') => {
            run_then_semicolon(2, 1, 7, char::is_ascii_digit)
                || (matches!(text.get(2), Some('x' | 'X'))
                    && run_then_semicolon(3, 1, 6, char::is_ascii_hexdigit))
        }
        Some(c) if c.is_ascii_alphabetic() => {
            run_then_semicolon(2, 1, 31, char::is_ascii_alphanumeric)
        }
        _ => false,
    }
}

/// The length of an ordered-list marker at the start of `text`
/// (`^[0-9]{1,9}[.)]($|[ \t])`), up to and including its delimiter, or `None`.
/// .NET's `$` also matches before a final line break, which this keeps.
fn ordered_marker(text: &[char]) -> Option<usize> {
    let digits = text.iter().take_while(|c| c.is_ascii_digit()).count();
    if !(1..=9).contains(&digits) || !matches!(text.get(digits), Some('.' | ')')) {
        return None;
    }

    let after = digits + 1;
    let ends = match text.get(after) {
        None | Some(' ' | '\t') => true,
        Some('\n') => after + 1 == text.len(),
        _ => false,
    };
    ends.then_some(after)
}

/// The part after a link's text: `](url "title")`, or `][label]` for a
/// reference link, whose url and title stay on its definition.
pub(crate) fn link_tail(
    link_url: Option<&str>,
    link_title: Option<&str>,
    link_label: Option<&str>,
) -> String {
    if let Some(label) = link_label {
        return format!("][{label}]");
    }

    let mut tail = String::from("](");
    tail.push_str(&destination(link_url.unwrap_or("")));
    if let Some(title) = link_title {
        tail.push_str(" \"");
        let title: Vec<char> = title.chars().collect();
        for (i, &c) in title.iter().enumerate() {
            if c == '\r' || c == '\n' {
                // A title cannot hold a blank line; a space keeps it one title.
                tail.push(' ');
                continue;
            }

            if c == '&' && entity_start(&title[i..]) {
                tail.push_str("&amp;");
                continue;
            }

            if c == '"' || c == '\\' {
                tail.push('\\');
            }

            tail.push(c);
        }

        tail.push('"');
    }

    tail.push(')');
    tail
}

/// A link destination as markdown.  The url is decoded (escapes and entities
/// resolved), so backslashes and entity-like text are escaped again; spaces,
/// parentheses or an empty url need the `<...>` form.
pub(crate) fn destination(url: &str) -> String {
    let url: Vec<char> = url.chars().collect();
    let pointy = url.is_empty()
        || url.iter().any(|&c| {
            c == ' '
                || c == '('
                || c == ')'
                || c == '<'
                || (c.is_control() && c != '\r' && c != '\n')
        });
    let mut destination = String::from(if pointy { "<" } else { "" });
    for (i, &c) in url.iter().enumerate() {
        if c == '\r' || c == '\n' {
            // A destination cannot span lines; percent-encoding keeps the same url.
            destination.push_str(if c == '\r' { "%0D" } else { "%0A" });
            continue;
        }

        if c == '&' && entity_start(&url[i..]) {
            destination.push_str("&amp;");
            continue;
        }

        let escape = c == '\\' || (pointy && (c == '<' || c == '>'));
        if escape {
            destination.push('\\');
        }

        destination.push(c);
    }

    if pointy {
        destination.push('>');
    }
    destination
}

/// A code span.  A span closes at the first backtick run of its own length, so
/// the fence differs in length from every run inside; CommonMark strips one
/// space from each side when both ends have one, and a leading or trailing
/// backtick would join the fence, so those cases get one space of padding.
pub(crate) fn append_code(code: &str, markdown: &mut String) {
    // An empty code span cannot be written; it renders as nothing anyway.
    if code.is_empty() {
        return;
    }

    let chars: Vec<char> = code.chars().collect();
    let mut runs = std::collections::HashSet::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        while i < chars.len() && chars[i] == '`' {
            i += 1;
        }

        if i > start {
            runs.insert(i - start);
        } else {
            i += 1;
        }
    }

    let mut fence_length = 1;
    while runs.contains(&fence_length) {
        fence_length += 1;
    }

    let fence = "`".repeat(fence_length);
    let first = chars[0];
    let last = chars[chars.len() - 1];
    let all_spaces = chars.iter().all(|&c| c == ' ');
    let pad = first == '`' || last == '`' || (!all_spaces && first == ' ' && last == ' ');
    let padding = if pad { " " } else { "" };
    markdown.push_str(&fence);
    markdown.push_str(padding);
    markdown.push_str(code);
    markdown.push_str(padding);
    markdown.push_str(&fence);
}

/// Appends plain text so it reads back as exactly that text: characters that
/// could start markdown syntax are backslash-escaped where they would (not
/// everywhere, so ordinary prose stays readable).  Runs hold decoded text, so
/// without this "&lt;b&gt;" typed as text would become live HTML.
pub(crate) fn append_escaped(text: &str, markdown: &mut String, table_cell: bool) {
    let mut text: Vec<char> = text.chars().collect();
    if markdown.is_empty() || markdown.ends_with('\n') {
        // Leading spaces never render, and four of them would make the line an indented code block.
        let lead = text.iter().take_while(|&&c| c == ' ' || c == '\t').count();
        text.drain(..lead);
        if text.is_empty() {
            return;
        }

        text = append_line_start(text, markdown);
    }

    for i in 0..text.len() {
        let previous = markdown.chars().next_back().unwrap_or('\n');
        let next = text.get(i + 1).copied().unwrap_or('\0');
        if needs_escape(&text, i, previous, next) || (table_cell && text[i] == '|') {
            markdown.push('\\');
        }

        markdown.push(text[i]);
    }
}

/// Escapes what only matters as a line's first characters (headings, quotes,
/// list markers, setext underlines) and returns the rest of the text.
fn append_line_start(text: Vec<char>, markdown: &mut String) -> Vec<char> {
    let first = text[0];
    if matches!(first, '#' | '>' | '-' | '+' | '=') {
        markdown.push('\\');
        markdown.push(first);
        return text[1..].to_vec();
    }

    if let Some(marker) = ordered_marker(&text) {
        let delimiter = marker - 1;
        markdown.extend(&text[..delimiter]);
        markdown.push('\\');
        markdown.push(text[delimiter]);
        return text[delimiter + 1..].to_vec();
    }

    text
}

fn needs_escape(text: &[char], i: usize, previous: char, next: char) -> bool {
    match text[i] {
        // The emphasis extras: ~sub~ and ^sup^ need only one character each.
        '\\' | '*' | '`' | '[' | ']' | '~' | '^' => true,

        // '_' never makes emphasis inside a word, so snake_case stays readable.
        '_' => !is_letter_or_digit(previous) || !is_letter_or_digit(next),

        // Only these can start inline HTML or an autolink.
        '<' => next == '\0' || is_letter(next) || next == '/' || next == '!' || next == '?',

        '&' => entity_start(&text[i..]),

        // ==marked== and ++inserted++.
        '=' | '+' => previous == text[i] || next == text[i],

        // The AutoLinks extension would turn http://... and mailto:... into links.
        ':' => (next == '/' && text.get(i + 2) == Some(&'/')) || ends_with_word(text, i, "mailto"),

        '.' => ends_with_word(text, i, "www"),

        _ => false,
    }
}

/// True when the text just before `i` is `word` at the start of a word.
fn ends_with_word(text: &[char], i: usize, word: &str) -> bool {
    let word: Vec<char> = word.chars().collect();
    let Some(start) = i.checked_sub(word.len()) else {
        return false;
    };
    text[start..i] == word[..] && (start == 0 || !is_letter_or_digit(text[start - 1]))
}
