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
//! the editor agree on them.  `parse.rs` calls [`pieces`] on each text event.

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
}

/// `text` split into plain text and extension spans, the spans styled from
/// `style` and `link`, the style and link around them.  Inside a link, a bare
/// URL is plain link text (Markdig does not autolink inside a link).
pub(super) fn pieces(text: &str, style: MarkdownRunStyle, link: Option<&str>) -> Vec<Piece> {
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
                }),
            },
            Some(InlineAtomKind::Autolink) if link.is_none() => Piece {
                extension: Some(Extension {
                    style,
                    link: Some(autolink_url(&piece)),
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

/// The URL a bare link opens: Markdig's AutoLinks extension prefixes a
/// `www.` link with `http://`.
fn autolink_url(text: &str) -> String {
    if text.len() >= 4 && text[..4].eq_ignore_ascii_case("www.") {
        format!("http://{text}")
    } else {
        text.to_string()
    }
}
