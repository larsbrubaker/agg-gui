//! Folds the viewer's alignment storage form (`<div align="center">`, blank
//! line, paragraphs and headings, blank line, `</div>`) into
//! [`RichBlock::alignment`] on the blocks inside.  Ports agg-sharp's
//! `RichAlignGroups.cs`.
//!
//! The parser reads the two tags as separate HTML blocks (an HTML block ends
//! at a blank line), so it first sees them as Raw blocks around the content;
//! this pass removes them, keeping their bytes on a shared [`RichAlignGroup`]
//! set on every block inside.  agg-sharp matches the tags with regular
//! expressions; the matchers here accept the same forms.

use super::block::{RichAlignment, RichBlock, RichBlockKind};
use super::groups::{shared, RichAlignGroup};
use super::markdown_parser;

/// Folds the alignment groups in `blocks`.  `html_blocks[i]` is true when
/// block `i` came from an HTML block: only those are tags; the same text as
/// indented code or inside a list is not.
pub(super) fn fold(blocks: &mut Vec<RichBlock>, mut html_blocks: Vec<bool>) {
    // Depth of plain or unmatched <div>s around the current block. A group inside another div
    // stays Raw, so an edit can never regenerate an inner wrapper and leave the outer one unbalanced.
    let mut div_depth = 0usize;
    let mut i = 0;
    while i < blocks.len() {
        if !html_blocks[i] {
            i += 1;
            continue;
        }
        if is_close_tag(&blocks[i], html_blocks[i]) {
            div_depth = div_depth.saturating_sub(1);
            i += 1;
            continue;
        }
        let html = blocks[i].original_source.trim().to_string();
        let open = open_tag(&html);
        let close = match open {
            Some(_) if div_depth == 0 => matching_close(blocks, i, &html_blocks),
            _ => None,
        };
        let (Some(alignment), Some(close)) = (open, close) else {
            let lower = html.to_ascii_lowercase();
            if lower.starts_with("<div") && !lower.contains("</div") {
                div_depth += 1;
            }
            i += 1;
            continue;
        };
        let group = shared(RichAlignGroup {
            alignment,
            open_source: blocks[i].original_source.clone() + &blocks[i + 1].separator_before,
            close_source: blocks[close].separator_before.clone() + &blocks[close].original_source,
            original_member_count: close - i - 1,
            one_line_source: None,
        });
        blocks[i + 1].separator_before = blocks[i].separator_before.clone();
        for inner in &mut blocks[i + 1..close] {
            inner.alignment = alignment;
            inner.align_group = Some(group.clone());
        }
        blocks.remove(close);
        html_blocks.remove(close);
        blocks.remove(i);
        html_blocks.remove(i);
        // Resume after the group's last block.
        i = close - 1;
    }
    fold_one_line(blocks, &html_blocks);
}

/// Makes each one-line aligned element (`<div align="center">text</div>`, a
/// `<p>` or an `<h1>`..`<h6>` the same way) an aligned Paragraph, or a Heading
/// for h1-h6, instead of a Raw block the rich view would show as nothing.  The
/// line goes on a one-member [`RichAlignGroup`] (`one_line_source`), so an
/// untouched block writes back its exact line; once edited or re-aligned it is
/// written in the regenerated wrapper form like any aligned block.  Runs after
/// the multi-line fold, so an element inside an aligned wrapper is not taken
/// into it.
fn fold_one_line(blocks: &mut [RichBlock], html_blocks: &[bool]) {
    for (block, &html) in blocks.iter_mut().zip(html_blocks) {
        if !html {
            continue;
        }
        let Some((tag, alignment, inner)) = one_line_element(&block.original_source) else {
            continue;
        };
        let inner = inner.trim().to_string();
        if inner.is_empty() || has_block_tag(&inner) {
            continue;
        }
        // The inner text must read as exactly one paragraph of itself ("# x" or "1. x" would not).
        let inner_document = markdown_parser::parse(&inner);
        let [only] = inner_document.blocks.as_slice() else {
            continue;
        };
        if only.kind != RichBlockKind::Paragraph || only.original_source != inner {
            continue;
        }
        let heading = tag.len() == 2 && tag.as_bytes()[0].eq_ignore_ascii_case(&b'h');
        block.kind = if heading {
            RichBlockKind::Heading
        } else {
            RichBlockKind::Paragraph
        };
        block.heading_level = if heading { tag.as_bytes()[1] - b'0' } else { 0 };
        block.inlines = only.inlines.clone();
        // The block's own source is its markdown, as for any clean block (a heading split off the
        // group is still written from it); the group keeps the exact line for while it is intact.
        let line = std::mem::take(&mut block.original_source);
        block.original_source = if heading {
            format!("{} {inner}", "#".repeat(block.heading_level as usize))
        } else {
            inner
        };
        block.alignment = alignment;
        block.align_group = Some(shared(RichAlignGroup {
            alignment,
            one_line_source: Some(line),
            original_member_count: 1,
            open_source: String::new(),
            close_source: String::new(),
        }));
    }
}

/// The index of the `</div>` closing the group opened at `open`, or `None`
/// when the group is empty, unclosed or holds anything but paragraphs and
/// headings (lists, quotes and Raw blocks are not aligned in the rich view).
fn matching_close(blocks: &[RichBlock], open: usize, html_blocks: &[bool]) -> Option<usize> {
    let mut index = open + 1;
    while index < blocks.len()
        && matches!(
            blocks[index].kind,
            RichBlockKind::Paragraph | RichBlockKind::Heading
        )
    {
        index += 1;
    }
    let closed = index > open + 1
        && index < blocks.len()
        && is_close_tag(&blocks[index], html_blocks[index]);
    closed.then_some(index)
}

fn is_close_tag(block: &RichBlock, html: bool) -> bool {
    html && block.original_source.trim().eq_ignore_ascii_case("</div>")
}

/// Skips ASCII whitespace from `at`.
fn skip_ws(text: &str, at: usize) -> usize {
    at + text[at..].len() - text[at..].trim_start().len()
}

/// Reads `align = "value"` (quotes `"`, `'` or none) at `at`, returning the
/// alignment and where the attribute ends.
fn align_attribute(text: &str, at: usize) -> Option<(RichAlignment, usize)> {
    let lower = text.to_ascii_lowercase();
    let at = skip_ws(text, at);
    if !lower[at..].starts_with("align") {
        return None;
    }
    let at = skip_ws(text, at + 5);
    if !text[at..].starts_with('=') {
        return None;
    }
    let at = skip_ws(text, at + 1);
    let quote = text[at..]
        .chars()
        .next()
        .filter(|c| *c == '"' || *c == '\'');
    let value_start = at + quote.map_or(0, |_| 1);
    let (alignment, word) = [
        (RichAlignment::Left, "left"),
        (RichAlignment::Center, "center"),
        (RichAlignment::Right, "right"),
    ]
    .into_iter()
    .find(|(_, word)| lower[value_start..].starts_with(word))?;
    let mut end = value_start + word.len();
    if let Some(q) = quote {
        if !text[end..].starts_with(q) {
            return None;
        }
        end += 1;
    }
    Some((alignment, end))
}

/// The alignment of a bare `<div align=...>` tag on its own; any other
/// attribute or content is not a form the writer regenerates.
fn open_tag(html: &str) -> Option<RichAlignment> {
    let lower = html.to_ascii_lowercase();
    if !lower.starts_with("<div") || !html.ends_with('>') {
        return None;
    }
    let after = 4;
    if !html[after..].starts_with(|c: char| c.is_ascii_whitespace()) {
        return None;
    }
    let (alignment, end) = align_attribute(html, after)?;
    (html[end..html.len() - 1].trim().is_empty()).then_some(alignment)
}

/// The tag name, alignment and inner text of a one-line aligned element:
/// `<tag align=...>inner</tag>` on one line with only the align attribute,
/// for `div`, `p` and `h1`..`h6`.
fn one_line_element(source: &str) -> Option<(String, RichAlignment, &str)> {
    if source.contains(['\r', '\n']) {
        return None;
    }
    let lower = source.to_ascii_lowercase();
    let start = skip_ws(source, 0);
    if !source[start..].starts_with('<') {
        return None;
    }
    let name_end = lower[start + 1..]
        .find(|c: char| !c.is_ascii_alphanumeric())
        .map(|i| i + start + 1)?;
    let tag = &lower[start + 1..name_end];
    let valid = tag == "div"
        || tag == "p"
        || (tag.len() == 2 && tag.starts_with('h') && (b'1'..=b'6').contains(&tag.as_bytes()[1]));
    if !valid || !source[name_end..].starts_with(|c: char| c.is_ascii_whitespace()) {
        return None;
    }
    let (alignment, end) = align_attribute(source, name_end)?;
    let close_bracket = skip_ws(source, end);
    if !source[close_bracket..].starts_with('>') {
        return None;
    }
    let inner_start = close_bracket + 1;
    // The closing tag ends the line, trailing whitespace aside.
    let trimmed = lower.trim_end();
    let close_end = trimmed.strip_suffix('>')?.trim_end();
    let close_start = close_end.rfind("</")?;
    if close_start < inner_start || &close_end[close_start + 2..] != tag {
        return None;
    }
    Some((
        tag.to_string(),
        alignment,
        &source[inner_start..close_start],
    ))
}

/// True when `inner` holds block-level HTML, which the rich view cannot edit
/// as one paragraph.
fn has_block_tag(inner: &str) -> bool {
    let lower = inner.to_ascii_lowercase();
    const TAGS: [&str; 15] = [
        "div",
        "p",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "table",
        "ul",
        "ol",
        "li",
        "blockquote",
        "pre",
        "hr",
    ];
    lower.match_indices('<').any(|(at, _)| {
        let rest = lower[at + 1..]
            .strip_prefix('/')
            .unwrap_or(&lower[at + 1..]);
        TAGS.iter().any(|tag| {
            rest.starts_with(tag)
                && !rest[tag.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
        })
    })
}
