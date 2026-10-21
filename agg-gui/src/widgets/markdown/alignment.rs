//! GitHub-style alignment HTML for `MarkdownView`, ported from MarkdigAgg's
//! `AggHtmlBlockRenderer` and `AggRenderer.BlockAlignment`.
//!
//! The markdown editor stores an aligned block as `<div align="center">`,
//! blank line, markdown, blank line, `</div>`.  An HTML block ends at a blank
//! line (in pulldown-cmark as in Markdig), so that arrives as an opening HTML
//! block, ordinary blocks, and a closing HTML block: the open and close tags
//! push and pop an [`AlignStack`], whose top aligns the paragraphs and
//! headings `parse.rs` emits in between.  The one-line form GitHub READMEs use
//! (`<p align="center">text</p>`, `<h2 align="right">Title</h2>`,
//! `<div align="center">text</div>`) shows its text as one aligned row.
//! Every other HTML block shows nothing, as before.
//!
//! C# matches these with regular expressions; they are hand-written here
//! (this crate has no regex dependency) and follow the same patterns, which
//! each function quotes.  `layout.rs` shifts an aligned flow's rows
//! ([`align_rows`]); `introspect.rs` reports each row's alignment.

use pulldown_cmark::{Event as MdEvent, Parser, Tag, TagEnd};

use super::{InlineItem, LayoutItem, LineRun, LineStyle, MarkdownRunStyle, ParagraphItem};

/// How a paragraph or heading sits across the view's width (C#'s
/// `ContentHAnchor` on `ParagraphX` / `HeadingRowX`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkdownAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// C# `AggRenderer`'s `blockAlignments` stack.
#[derive(Default)]
pub(super) struct AlignStack(Vec<MarkdownAlign>);

impl AlignStack {
    /// C# `BlockAlignment`: the innermost open div's alignment, else left.
    pub(super) fn current(&self) -> MarkdownAlign {
        self.0.last().copied().unwrap_or_default()
    }

    /// A stray `</div>` with no open div is ignored, keeping the rest of the
    /// document as written (C# `PopBlockAlignment`).
    fn pop(&mut self) {
        self.0.pop();
    }
}

/// Collects the text of each HTML block from its events.
#[derive(Default)]
pub(super) struct HtmlBlocks {
    text: Option<String>,
}

/// What an event means to [`HtmlBlocks`].
pub(super) enum HtmlFeed {
    /// Not part of an HTML block.
    Pass,
    /// Part of an HTML block still being read.
    Consumed,
    /// The end of an HTML block, with its whole text.
    Block(String),
}

impl HtmlBlocks {
    pub(super) fn feed(&mut self, ev: &MdEvent<'_>) -> HtmlFeed {
        match ev {
            MdEvent::Start(Tag::HtmlBlock) => {
                self.text = Some(String::new());
                HtmlFeed::Consumed
            }
            MdEvent::End(TagEnd::HtmlBlock) => {
                HtmlFeed::Block(self.text.take().unwrap_or_default())
            }
            MdEvent::Html(t) | MdEvent::Text(t) if self.text.is_some() => {
                if let Some(text) = self.text.as_mut() {
                    text.push_str(t);
                }
                HtmlFeed::Consumed
            }
            _ => HtmlFeed::Pass,
        }
    }
}

/// C# `AggHtmlBlockRenderer.Write`: apply one HTML block. Opening and
/// closing divs change `aligns`; a one-line aligned element pushes its row
/// (and the spacing after it) onto `out`.
pub(super) fn write_html_block(
    html: &str,
    aligns: &mut AlignStack,
    out: &mut Vec<ParagraphItem>,
    indent: f64,
    quote: bool,
) {
    let html = html.trim();
    if let Some(open) = open_tag(html).filter(|open| open.tag == "div" && open.rest.is_empty()) {
        // A div without an align attribute inherits the alignment around it, so its
        // own </div> pops back to that alignment rather than ending an outer aligned div early.
        let align = parse_alignment(open.attributes).unwrap_or(aligns.current());
        aligns.0.push(align);
        return;
    }
    if is_div_close(html) {
        aligns.pop();
        return;
    }
    let Some((tag, attributes, inner)) = aligned_text_element(html) else {
        return;
    };
    let Some(align) = parse_alignment(attributes) else {
        return;
    };
    // Inner markup is shown as plain text: GitHub's one-line form rarely carries more than
    // a word or a link, and showing its text beats dropping the whole line.
    let text = html_decode(&strip_tags(inner));
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    // An element holding only markup (a centered <img> logo) would otherwise add an empty padded row.
    if text.is_empty() {
        return;
    }
    let style = match tag.as_str() {
        "h1" => LineStyle::H1,
        "h2" => LineStyle::H2,
        "h3" => LineStyle::H3,
        "p" | "div" => LineStyle::Body,
        _ => LineStyle::H4,
    };
    let level = tag
        .strip_prefix('h')
        .and_then(|l| l.parse().ok())
        .unwrap_or(0);
    out.push(ParagraphItem::Flow {
        items: vec![InlineItem::Text {
            text,
            link: None,
            code: false,
            style: MarkdownRunStyle::default(),
            glue: false,
        }],
        style,
        indent,
        quote,
        align,
        level,
        anchor: None,
    });
    if !matches!(out.last(), Some(ParagraphItem::Spacer)) {
        out.push(ParagraphItem::Spacer);
    }
}

/// Give every paragraph and heading in `items` the alignment `align`.
pub(super) fn stamp(items: &mut [ParagraphItem], align: MarkdownAlign) {
    for item in items {
        if let ParagraphItem::Flow { align: a, .. } = item {
            *a = align;
        }
    }
}

/// Shift each row of an aligned flow across the `avail` width its rows wrapped in.
pub(super) fn align_rows(rows: &mut [LayoutItem], avail: f64, align: MarkdownAlign) {
    let factor = match align {
        MarkdownAlign::Left => return,
        MarkdownAlign::Center => 0.5,
        MarkdownAlign::Right => 1.0,
    };
    for row in rows {
        let LayoutItem::Line { runs, align: a, .. } = row else {
            continue;
        };
        *a = align;
        let end = runs
            .iter()
            .map(|run| match run {
                LineRun::Text { x, width, .. } | LineRun::Image { x, width, .. } => x + width,
            })
            .fold(0.0_f64, f64::max);
        let shift = ((avail - end) * factor).max(0.0);
        for run in runs.iter_mut() {
            match run {
                LineRun::Text { x, .. } | LineRun::Image { x, .. } => *x += shift,
            }
        }
    }
}

/// A start tag split as `^<(tag)(\s[^>]*)?>(rest)`, its name lowercased.
struct OpenTag<'a> {
    tag: String,
    attributes: &'a str,
    rest: &'a str,
}

fn open_tag(html: &str) -> Option<OpenTag<'_>> {
    let body = html.strip_prefix('<')?;
    let name_len = body
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(body.len());
    let tag = body[..name_len].to_ascii_lowercase();
    let after = &body[name_len..];
    let close = after.find('>')?;
    let attributes = &after[..close];
    // The name ends at `>` or at whitespace that begins the attributes.
    if !(attributes.is_empty() || attributes.starts_with(char::is_whitespace)) {
        return None;
    }
    Some(OpenTag {
        tag,
        attributes,
        rest: &after[close + 1..],
    })
}

/// `^</div\s*>$`, ignoring case.
fn is_div_close(html: &str) -> bool {
    html.get(..5)
        .is_some_and(|start| start.eq_ignore_ascii_case("</div"))
        && html[5..]
            .strip_suffix('>')
            .is_some_and(|gap| gap.trim().is_empty())
}

/// `^<(p|div|h[1-6])(\s[^>]*)?>((?:(?!</?\1\b).)*)</\1\s*>$`, ignoring case:
/// the tag, its attributes and its inner HTML. The inner HTML may not open
/// or close the same tag, so `<p align="center">a</p><p>b</p>` is not read
/// as one element gluing "a" and "b"; that block shows nothing.
fn aligned_text_element(html: &str) -> Option<(String, &str, &str)> {
    let open = open_tag(html)?;
    let tag_ok = matches!(open.tag.as_str(), "p" | "div")
        || (open.tag.len() == 2
            && open.tag.starts_with('h')
            && matches!(open.tag.as_bytes()[1], b'1'..=b'6'));
    if !tag_ok {
        return None;
    }
    let before_gt = open.rest.strip_suffix('>')?.trim_end();
    let close = format!("</{}", open.tag);
    let split = before_gt.len().checked_sub(close.len())?;
    if !before_gt.is_char_boundary(split) || !before_gt[split..].eq_ignore_ascii_case(&close) {
        return None;
    }
    let inner = &before_gt[..split];
    if mentions_tag(inner, &open.tag) {
        return None;
    }
    Some((open.tag, open.attributes, inner))
}

/// Whether `inner` holds `<tag` or `</tag` followed by a word boundary.
fn mentions_tag(inner: &str, tag: &str) -> bool {
    let lower = inner.to_ascii_lowercase();
    for (at, _) in lower.match_indices('<') {
        let after = lower[at + 1..]
            .strip_prefix('/')
            .unwrap_or(&lower[at + 1..]);
        if let Some(next) = after.strip_prefix(tag) {
            if !next.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                return true;
            }
        }
    }
    false
}

/// C# `ParseAlignment`: `(?:^|\s)align\s*=\s*["']?(left|center|right)\b`,
/// ignoring case. The start or whitespace before `align` keeps
/// `data-align="center"` and similar attributes from counting.
fn parse_alignment(attributes: &str) -> Option<MarkdownAlign> {
    let lower = attributes.to_ascii_lowercase();
    for (at, _) in lower.match_indices("align") {
        if at > 0 && !lower[..at].ends_with(char::is_whitespace) {
            continue;
        }
        let Some(value) = lower[at + 5..].trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        let value = value.strip_prefix(['"', '\'']).unwrap_or(value);
        for (word, align) in [
            ("left", MarkdownAlign::Left),
            ("center", MarkdownAlign::Center),
            ("right", MarkdownAlign::Right),
        ] {
            if let Some(next) = value.strip_prefix(word) {
                if !next.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                    return Some(align);
                }
            }
        }
    }
    None
}

/// `<[^>]*>` replaced with nothing (an unclosed `<` stays as text).
fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        let Some(gt) = rest[lt..].find('>') else {
            break;
        };
        out.push_str(&rest[..lt]);
        rest = &rest[lt + gt + 1..];
    }
    out.push_str(rest);
    out
}

/// C# `WebUtility.HtmlDecode`: each `&name;` / `&#n;` / `&#xh;` reference
/// replaced by its character. pulldown-cmark holds the HTML entity table, so
/// each reference is decoded by letting it read the reference alone; text
/// that is no reference stays as written.
fn html_decode(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let reference = tail[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .filter(|&end| end > 0 && tail[1 + end..].starts_with(';'))
            .map(|end| &tail[..end + 2]);
        match reference.and_then(decode_reference) {
            Some(decoded) => {
                out.push_str(&decoded);
                rest = &tail[reference.map_or(1, str::len)..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn decode_reference(reference: &str) -> Option<String> {
    let mut decoded = String::new();
    for ev in Parser::new(reference) {
        if let MdEvent::Text(t) = ev {
            decoded.push_str(&t);
        }
    }
    (!decoded.is_empty() && decoded != reference).then_some(decoded)
}
