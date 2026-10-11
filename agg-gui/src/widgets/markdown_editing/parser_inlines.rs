//! The inline half of agg-sharp's `RichMarkdownParser` (`AddInlines` and its
//! helpers, plus `HtmlStyleTags.Pair`): a paragraph's, heading's or table
//! cell's inline nodes ([`super::parse_tree`]) become styled runs and atoms.
//!
//! The viewer's Markdig pipeline also reads `^sup^`, `==mark==`, `++ins++`,
//! `~sub~` and bare `www.`/`http://` links, which become Raw and Autolink atoms
//! of their exact source.  pulldown-cmark reads none of them (it takes `~x~`
//! as strikethrough), so this module finds them in the text it hands back:
//! [`scan_extensions`] over each verbatim run of text, and a single-tilde
//! strikethrough read as a Raw subscript atom.

use pulldown_cmark::LinkType;

use super::inline::{InlineAtomKind, RichInline};
use super::inlines;
use super::parse_tree::{Node, NodeKind};

/// The style the inlines inside a container inherit.
#[derive(Clone, Debug, Default, PartialEq)]
struct Style {
    bold: bool,
    italic: bool,
    strike: bool,
    link_url: Option<String>,
    link_title: Option<String>,
    link_label: Option<String>,
}

/// Reads inline nodes against the source their ranges index.
pub(super) struct InlineReader<'a> {
    pub source: &'a str,
    /// Keep each soft line break as a hard break (pasted plain text).
    pub soft_breaks_are_hard: bool,
}

impl InlineReader<'_> {
    /// The model inlines of a leaf's inline children, adjacent same-style runs
    /// merged.
    pub fn inlines_of(&self, leaf: &Node) -> Vec<RichInline> {
        let mut out = Vec::new();
        self.add_inlines(&leaf.children, Style::default(), &mut out);
        inlines::merge_adjacent(&mut out);
        out
    }

    fn slice(&self, node: &Node) -> &str {
        &self.source[node.range.clone()]
    }

    fn add_inlines(&self, children: &[Node], mut style: Style, out: &mut Vec<RichInline>) {
        let paired = self.pair_style_tags(children);
        let mut outer: Vec<Style> = Vec::new();
        let mut index = 0;
        while index < children.len() {
            let node = &children[index];
            // A paired <strong>/<em>/<del> (or <b>/<i>/<s>/<strike>) styles what lies between, so
            // text the writer had to wrap in tags (where ** would not read as bold) stays editable.
            if let Some(opens) = paired[index] {
                if opens {
                    outer.push(style.clone());
                    match style_tag_name(self.slice(node))
                        .unwrap_or_default()
                        .as_str()
                    {
                        "strong" | "b" => style.bold = true,
                        "em" | "i" => style.italic = true,
                        _ => style.strike = true,
                    }
                } else if let Some(previous) = outer.pop() {
                    style = previous;
                }
                index += 1;
                continue;
            }

            match &node.kind {
                NodeKind::Text(_) => {
                    // Adjacent pieces of text are read as one, so an extension's delimiters split
                    // across pulldown-cmark's text events are still found.
                    let mut end = index;
                    while end + 1 < children.len()
                        && matches!(children[end + 1].kind, NodeKind::Text(_))
                    {
                        end += 1;
                    }
                    self.add_text(&children[index..=end], &style, out);
                    index = end + 1;
                    continue;
                }
                NodeKind::Code(content) => {
                    out.push(styled(RichInline::run(content.clone()).code(), &style));
                }
                NodeKind::HardBreak => out.push(styled(
                    RichInline::atom(InlineAtomKind::HardBreak, self.hard_break_source(node)),
                    &style,
                )),
                NodeKind::SoftBreak if self.soft_breaks_are_hard => out.push(styled(
                    RichInline::atom(InlineAtomKind::HardBreak, "\\\n"),
                    &style,
                )),
                // A soft break renders as a space; the editor wraps lines itself.
                NodeKind::SoftBreak => out.push(styled(RichInline::run(" "), &style)),
                NodeKind::InlineHtml => out.push(self.atom(&style, InlineAtomKind::Html, node)),
                NodeKind::Image => out.push(self.atom(&style, InlineAtomKind::Image, node)),
                NodeKind::Link {
                    link_type: LinkType::Autolink | LinkType::Email,
                    ..
                } => out.push(self.atom(&style, InlineAtomKind::Autolink, node)),
                NodeKind::Link {
                    link_type,
                    url,
                    title,
                    id,
                } => {
                    let reference = !matches!(link_type, LinkType::Inline);
                    // A reference link's title lives on its definition, which stays put; only
                    // inline links carry one.
                    let inner = Style {
                        link_url: Some(url.clone()),
                        link_title: (!reference && !title.is_empty()).then(|| title.clone()),
                        link_label: reference.then(|| id.clone()),
                        ..style.clone()
                    };
                    self.add_inlines(&node.children, inner, out);
                }
                NodeKind::Emphasis => {
                    let inner = Style {
                        italic: true,
                        ..style.clone()
                    };
                    self.add_inlines(&node.children, inner, out);
                }
                NodeKind::Strong => {
                    let inner = Style {
                        bold: true,
                        ..style.clone()
                    };
                    self.add_inlines(&node.children, inner, out);
                }
                // `~x~` is Markdig's subscript, which the model does not hold; `~~x~~` strikes.
                NodeKind::Strikethrough if self.slice(node).starts_with("~~") => {
                    let inner = Style {
                        strike: true,
                        ..style.clone()
                    };
                    self.add_inlines(&node.children, inner, out);
                }
                NodeKind::TaskMarker => {
                    out.push(self.atom(&style, InlineAtomKind::Raw, node));
                    // pulldown-cmark leaves the spaces after a task checkbox out of the item's
                    // text, where Markdig keeps them in the paragraph.
                    if let Some(next) = children.get(index + 1) {
                        let gap =
                            &self.source[node.range.end..next.range.start.max(node.range.end)];
                        if !gap.is_empty() && gap.chars().all(|c| c == ' ' || c == '\t') {
                            out.push(styled(RichInline::run(gap), &style));
                        }
                    }
                }
                _ => out.push(self.atom(&style, InlineAtomKind::Raw, node)),
            }
            index += 1;
        }
    }

    // Atoms carry the style around them too, so a rewrite can wrap an image in **bold** or a link
    // again.
    fn atom(&self, style: &Style, kind: InlineAtomKind, node: &Node) -> RichInline {
        styled(RichInline::atom(kind, self.slice(node)), style)
    }

    /// Text pieces: each source-contiguous verbatim stretch is scanned for the
    /// viewer's extensions; decoded text (escapes, entities) is a plain run.
    fn add_text(&self, pieces: &[Node], style: &Style, out: &mut Vec<RichInline>) {
        let mut stretch: Option<std::ops::Range<usize>> = None;
        let flush = |stretch: &mut Option<std::ops::Range<usize>>, out: &mut Vec<RichInline>| {
            if let Some(range) = stretch.take() {
                for (text, raw) in scan_extensions(self.source, range) {
                    out.push(match raw {
                        Some(kind) => styled(RichInline::atom(kind, text), style),
                        None => styled(RichInline::run(text), style),
                    });
                }
            }
        };
        for piece in pieces {
            let NodeKind::Text(content) = &piece.kind else {
                continue;
            };
            let verbatim = self.slice(piece) == content;
            match (&mut stretch, verbatim) {
                (Some(range), true) if range.end == piece.range.start => {
                    range.end = piece.range.end
                }
                (_, true) => {
                    flush(&mut stretch, out);
                    stretch = Some(piece.range.clone());
                }
                (_, false) => {
                    flush(&mut stretch, out);
                    out.push(styled(RichInline::run(content.clone()), style));
                }
            }
        }
        flush(&mut stretch, out);
    }

    /// The hard break's whole source: the trailing spaces or backslash that
    /// make it hard, and both characters of a CRLF.
    fn hard_break_source(&self, node: &Node) -> String {
        let bytes = self.source.as_bytes();
        let mut start = node.range.start;
        if bytes.get(start) != Some(&b'\\') {
            if start > 0 && bytes[start - 1] == b'\\' {
                start -= 1;
            } else {
                while start > 0 && bytes[start - 1] == b' ' {
                    start -= 1;
                }
            }
        }
        let mut end = node.range.end;
        if end < bytes.len() && bytes[end - 1] == b'\r' && bytes[end] == b'\n' {
            end += 1;
        }
        self.source[start..end].to_string()
    }

    /// For each child, `Some(true)` for a style tag opening a pair,
    /// `Some(false)` for the tag closing it, `None` otherwise.  Unpaired and
    /// crossed tags are left out (`HtmlStyleTags.Pair`).
    fn pair_style_tags(&self, children: &[Node]) -> Vec<Option<bool>> {
        let mut paired = vec![None; children.len()];
        let mut open: Vec<(usize, String)> = Vec::new();
        for (index, node) in children.iter().enumerate() {
            if node.kind != NodeKind::InlineHtml {
                continue;
            }
            let tag = self.slice(node);
            let Some(name) = style_tag_name(tag) else {
                continue;
            };
            if !tag.starts_with("</") {
                open.push((index, name));
            } else if open.last().is_some_and(|(_, open_name)| *open_name == name) {
                let (opener, _) = open.pop().unwrap_or_default();
                paired[opener] = Some(true);
                paired[index] = Some(false);
            } else {
                // Crossed tags: the open ones can no longer close in order, so they stay unknown HTML.
                open.clear();
            }
        }
        paired
    }
}

/// The lower-case name of a bare style tag (`<b>`, `</em>`...), or `None` for
/// any other HTML; a tag with attributes stays unknown HTML.
fn style_tag_name(tag: &str) -> Option<String> {
    let inner = tag.strip_prefix('<')?.strip_suffix('>')?;
    let name = inner
        .strip_prefix('/')
        .unwrap_or(inner)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "strong" | "b" | "em" | "i" | "del" | "s" | "strike"
    )
    .then_some(name)
}

fn styled(mut inline: RichInline, style: &Style) -> RichInline {
    inline.bold = style.bold;
    inline.italic = style.italic;
    inline.strike = style.strike;
    inline.link_url = style.link_url.clone();
    inline.link_title = style.link_title.clone();
    inline.link_label = style.link_label.clone();
    inline
}

/// Splits `source[range]` into plain text and the atoms of the viewer's
/// extensions pulldown-cmark leaves as text: `^sup^`, `==mark==`, `++ins++`
/// (Raw) and bare `www.`, `http://`, `https://`, `ftp://` and `mailto:` links
/// (Autolink).  Each piece is `(text, Some(atom kind))` or `(text, None)`.
pub(super) fn scan_extensions(
    source: &str,
    range: std::ops::Range<usize>,
) -> Vec<(String, Option<InlineAtomKind>)> {
    let bytes = source.as_bytes();
    let mut pieces = Vec::new();
    let mut plain = range.start;
    let mut at = range.start;
    while at < range.end {
        let found = delimited(bytes, at, range.end, b"==")
            .or_else(|| delimited(bytes, at, range.end, b"++"))
            .or_else(|| delimited(bytes, at, range.end, b"^"))
            .map(|end| (end, InlineAtomKind::Raw))
            .or_else(|| {
                bare_link(source, at, range.end).map(|end| (end, InlineAtomKind::Autolink))
            });
        match found {
            Some((end, kind)) => {
                if plain < at {
                    pieces.push((source[plain..at].to_string(), None));
                }
                pieces.push((source[at..end].to_string(), Some(kind)));
                at = end;
                plain = end;
            }
            None => at += source[at..].chars().next().map_or(1, char::len_utf8),
        }
    }
    if plain < range.end {
        pieces.push((source[plain..range.end].to_string(), None));
    }
    pieces
}

/// The end of a `delim`-wrapped span starting at `at`: the opener is not
/// followed by whitespace (or a third delimiter character), the closer not
/// preceded by whitespace, and the content is not empty.
fn delimited(bytes: &[u8], at: usize, end: usize, delim: &[u8]) -> Option<usize> {
    let n = delim.len();
    if at + n >= end || &bytes[at..at + n] != delim || (at > 0 && bytes[at - 1] == delim[0]) {
        return None;
    }
    let first = bytes[at + n];
    if first.is_ascii_whitespace() || first == delim[0] {
        return None;
    }
    let mut close = at + n + 1;
    while close + n <= end {
        if &bytes[close..close + n] == delim && !bytes[close - 1].is_ascii_whitespace() {
            let after = close + n;
            if after < end && bytes[after] == delim[0] {
                return None;
            }
            // A superscript holds no spaces (Markdig's `^` reads only a word).
            if n == 1 && bytes[at + 1..close].iter().any(u8::is_ascii_whitespace) {
                return None;
            }
            return Some(after);
        }
        close += 1;
    }
    None
}

/// The end of a bare link starting at `at` (Markdig's AutoLinks extension):
/// a `www.` domain or a scheme, preceded by the start of the text, whitespace
/// or `*_~(`, running to whitespace or `<`, with trailing punctuation left out.
fn bare_link(source: &str, at: usize, end: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if at > 0 && !(bytes[at - 1].is_ascii_whitespace() || b"*_~(".contains(&bytes[at - 1])) {
        return None;
    }
    let rest = &source[at..end];
    let prefix = ["www.", "http://", "https://", "ftp://", "mailto:"]
        .into_iter()
        .find(|p| rest.len() > p.len() && rest[..p.len()].eq_ignore_ascii_case(p))?;
    let mut stop = rest
        .find(|c: char| c.is_whitespace() || c == '<')
        .unwrap_or(rest.len());
    while stop > prefix.len() && "?!.,:*_~')".contains(rest[..stop].chars().last()?) {
        let last = rest[..stop].chars().last()?;
        if last == ')' && rest[..stop].matches('(').count() >= rest[..stop].matches(')').count() {
            break;
        }
        stop -= last.len_utf8();
    }
    let host = &rest[prefix.len()..stop];
    let domain_end = host.find(['/', '?', '#']).unwrap_or(host.len());
    let domain = &host[..domain_end];
    let valid = !domain.is_empty()
        && (prefix == "mailto:" || prefix != "www." || domain.contains('.'))
        && domain
            .chars()
            .all(|c| c.is_alphanumeric() || "-_.@".contains(c));
    valid.then_some(at + stop)
}
