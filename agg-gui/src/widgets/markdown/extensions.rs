//! The viewer's Markdig extensions pulldown-cmark leaves as text, rendered as
//! agg-sharp's MarkdigAgg renders them (`AggEmphasisInlineRenderer`,
//! `EmphasisInlineX.AddChild`, `AggLinkInlineRenderer`).
//!
//! C#'s pipeline is `UseEmphasisExtras().UseGridTables().UsePipeTables()
//! .UseTaskLists().UseAutoLinks()`.  `^sup^`, `++ins++` and `==mark==` have no
//! style of their own in MarkdigAgg yet and stay bold; `~sub~` is drawn
//! struck through, as its `~` delimiter makes it there (pulldown-cmark reads
//! a single-tilde span as strikethrough already); bare `www.`/`http://` links
//! are links.  The spans are found by the rich editor's reader's own scanner
//! (`markdown_editing::parser_inlines::scan_extensions`), so the viewer and
//! the editor agree on them.  `parse.rs` and `html_form.rs` call [`pieces`]
//! on each text event.

use crate::widgets::markdown_editing::parser_inlines::scan_extensions;
use crate::widgets::markdown_editing::InlineAtomKind;

use super::MarkdownRunStyle;

/// One piece of a text event: its text, and for an extension span the style
/// and link it renders with.
pub(super) struct Piece {
    pub text: String,
    /// `None` for plain text, which keeps the surrounding style and link.
    pub extension: Option<Extension>,
}

/// How an extension span renders.
pub(super) struct Extension {
    pub style: MarkdownRunStyle,
    pub link: Option<String>,
    /// The HTML element Markdig renders the span as (`sup`, `mark`, `ins`,
    /// `sub` or `a`), for the viewer's HTML form (`html_form.rs`).
    pub tag: &'static str,
}

/// `text` split into plain text and extension spans, the spans styled from
/// `style` and `link`, the style and link around them.  Inside a link, a bare
/// URL is plain link text (Markdig does not autolink inside a link).
///
/// `source[range]` is the Markdown the text event was read from.  A
/// character that was escaped (`\^`) or written as an entity (`&#94;`) there
/// is never a delimiter, as in Markdig; pulldown-cmark's text no longer shows
/// which ones were, so they are masked (see [`mask_escaped`]) while the text
/// is scanned.
pub(super) fn pieces(
    text: &str,
    source: &str,
    range: std::ops::Range<usize>,
    style: MarkdownRunStyle,
    link: Option<&str>,
) -> Vec<Piece> {
    let masked = mask_escaped(text, source, range);
    let mut pieces = unmasked_pieces(&masked, style, link);
    if masked != text {
        for piece in &mut pieces {
            piece.text = unmask(&piece.text);
            if let Some(extension) = piece.extension.as_mut() {
                extension.link = extension.link.as_deref().map(unmask);
            }
        }
    }
    pieces
}

/// The first private-use code point of the mask: an escaped ASCII character
/// `c` scans as `MASK + c`, which no delimiter or link scan matches.
const MASK: u32 = 0xE000;

/// `text` with each character `source[range]` escapes or writes as an entity
/// replaced by its mask.  pulldown-cmark starts a text event at an escaped
/// character, its backslash just before the range.  Inside the range the two
/// are walked side by side: a character that is the same in both is literal;
/// a backslash escape or an entity in the source stands for the text
/// characters up to the next one that matches again.
fn mask_escaped(text: &str, source: &str, range: std::ops::Range<usize>) -> String {
    let before = &source[..range.start];
    let backslashes = before.len() - before.trim_end_matches('\\').len();
    let escaped_first = backslashes % 2 == 1;
    let source = &source[range];
    if text == source && !escaped_first {
        return text.to_string();
    }
    let mask = |c: char| {
        if c.is_ascii() {
            char::from_u32(MASK + c as u32).unwrap_or(c)
        } else {
            c
        }
    };
    let mut out = String::with_capacity(text.len());
    let mut src = source;
    let mut chars = text.chars().peekable();
    if escaped_first {
        if let Some(c) = chars.next() {
            out.push(mask(c));
            src = src.get(c.len_utf8()..).unwrap_or("");
        }
    }
    while let Some(c) = chars.next() {
        if src.starts_with(c) {
            out.push(c);
            src = &src[c.len_utf8()..];
        } else if src.starts_with('\\') && src[1..].starts_with(c) {
            out.push(mask(c));
            src = &src[1 + c.len_utf8()..];
        } else if src.starts_with('&') {
            out.push(mask(c));
            src = src.find(';').map_or("", |end| &src[end + 1..]);
            // An entity may stand for more than one character.
            while let Some(&next) = chars.peek() {
                if src.starts_with(next) {
                    break;
                }
                out.push(mask(next));
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `text` with masked characters back as themselves.
fn unmask(text: &str) -> String {
    text.chars()
        .map(|c| match c as u32 {
            code @ MASK..=0xE07F => char::from_u32(code - MASK).unwrap_or(c),
            _ => c,
        })
        .collect()
}

fn unmasked_pieces(text: &str, style: MarkdownRunStyle, link: Option<&str>) -> Vec<Piece> {
    scan_extensions(text, 0..text.len())
        .into_iter()
        .map(|(piece, kind)| match kind {
            Some(InlineAtomKind::Raw) => Piece {
                text: strip_delimiters(&piece).to_string(),
                extension: Some(Extension {
                    style: MarkdownRunStyle {
                        bold: true,
                        ..style
                    },
                    link: link.map(str::to_string),
                    tag: delimiter_tag(&piece),
                }),
            },
            Some(InlineAtomKind::Autolink) if link.is_none() => Piece {
                extension: Some(Extension {
                    style,
                    link: Some(autolink_url(&piece)),
                    tag: "a",
                }),
                text: piece,
            },
            _ => Piece {
                text: piece,
                extension: None,
            },
        })
        .flat_map(|piece| match piece.extension {
            Some(_) => vec![piece],
            None => subscripts(&piece.text, style, link),
        })
        .collect()
}

/// Plain text split at the `~sub~` spans pulldown-cmark left as text (it
/// takes a single-tilde span for strikethrough only between word
/// boundaries, so `H~2~O` reaches here), each span struck through.
fn subscripts(text: &str, style: MarkdownRunStyle, link: Option<&str>) -> Vec<Piece> {
    let bytes = text.as_bytes();
    let plain = |text: &str| Piece {
        text: text.to_string(),
        extension: None,
    };
    let mut pieces = Vec::new();
    let mut plain_start = 0;
    let mut at = 0;
    while at < bytes.len() {
        let opens = bytes[at] == b'~'
            && (at == 0 || bytes[at - 1] != b'~')
            && bytes
                .get(at + 1)
                .is_some_and(|&b| b != b'~' && !b.is_ascii_whitespace());
        let close = opens
            .then(|| {
                (at + 2..bytes.len()).find(|&i| {
                    bytes[i] == b'~'
                        && !bytes[i - 1].is_ascii_whitespace()
                        && bytes.get(i + 1) != Some(&b'~')
                })
            })
            .flatten();
        let Some(close) = close else {
            at += 1;
            continue;
        };
        if plain_start < at {
            pieces.push(plain(&text[plain_start..at]));
        }
        pieces.push(Piece {
            text: text[at + 1..close].to_string(),
            extension: Some(Extension {
                style: MarkdownRunStyle {
                    strikethrough: true,
                    ..style
                },
                link: link.map(str::to_string),
                tag: "sub",
            }),
        });
        at = close + 1;
        plain_start = at;
    }
    if plain_start < bytes.len() || pieces.is_empty() {
        pieces.push(plain(&text[plain_start..]));
    }
    pieces
}

/// The content of `==x==`, `++x++` or `^x^`.
fn strip_delimiters(span: &str) -> &str {
    for delimiter in ["==", "++", "^"] {
        if let Some(inner) = span
            .strip_prefix(delimiter)
            .and_then(|s| s.strip_suffix(delimiter))
        {
            return inner;
        }
    }
    span
}

/// The element Markdig renders a `==x==`, `++x++` or `^x^` span as.
fn delimiter_tag(span: &str) -> &'static str {
    if span.starts_with("==") {
        "mark"
    } else if span.starts_with("++") {
        "ins"
    } else {
        "sup"
    }
}

/// The URL a bare link opens: Markdig's AutoLinks extension prefixes a
/// `www.` link with `http://`.
pub(super) fn autolink_url(text: &str) -> String {
    if text.len() >= 4 && text[..4].eq_ignore_ascii_case("www.") {
        format!("http://{text}")
    } else {
        text.to_string()
    }
}
