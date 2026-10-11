//! The HTML form of `MarkdownView`'s parse: what the viewer reads in a
//! Markdown source, written as HTML, so two sources can be compared for
//! "renders the same" the way agg-sharp's rich-editor tests compare Markdig's
//! HTML (`RichMarkdownWriterTests.Html`).
//!
//! It reads exactly what `parse.rs` reads: the same source rewrite (Markdig's
//! pipe and grid tables, `parse::viewer_source`), the same pulldown-cmark
//! options and the same extension spans (`extensions::pieces`), each written
//! as the element Markdig writes for it: `^x^` `<sup>`, `~x~` `<sub>`,
//! `==x==` `<mark>`, `++x++` `<ins>`, a bare link `<a href>`, `~~x~~` `<del>`,
//! and a grid table without a header row without `<thead>`.  The rest is
//! pulldown-cmark's own HTML writer.
//!
//! [`markdown_html`] then normalises the HTML as the C# tests' `Html()` does
//! ([`markdown_html_collapsed`] only collapses whitespace, as the code and
//! table tests' `Html()` does):
//! `<b>` reads as `<strong>`, `<i>` as `<em>`, `<s>` and `<strike>` as
//! `<del>`, and every whitespace run collapses to one space.

use pulldown_cmark::{html, CowStr, Event, Parser, Tag, TagEnd};

use super::extensions;
use super::parse::{viewer_source, OPTIONS};
use super::MarkdownRunStyle;

/// The viewer's parse of `markdown` as normalised HTML (see the module doc).
pub fn markdown_html(markdown: &str) -> String {
    collapse_whitespace(&normalise_style_tags(&raw_html(markdown)))
}

/// The viewer's parse of `markdown` as HTML with only its whitespace runs
/// collapsed, as `RichMarkdownWriterCodeTableTests.Html()` compares it.
pub fn markdown_html_collapsed(markdown: &str) -> String {
    collapse_whitespace(&raw_html(markdown))
}

/// The viewer's parse of `markdown` as HTML, before normalising (what
/// `styled_html.rs` styles for the clipboard).
pub(super) fn raw_html(markdown: &str) -> String {
    let (source, headerless) = viewer_source(markdown);
    let mut events: Vec<Event<'_>> = Vec::new();
    let mut code_depth = 0;
    let mut image_depth = 0;
    let mut links: Vec<String> = Vec::new();
    let mut strike_tags: Vec<&'static str> = Vec::new();
    let mut headerless_table = false;
    for (event, range) in Parser::new_ext(&source, OPTIONS).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                code_depth += 1;
                events.push(event);
            }
            Event::End(TagEnd::CodeBlock) => {
                code_depth -= 1;
                events.push(event);
            }
            Event::Start(Tag::Image { .. }) => {
                image_depth += 1;
                events.push(event);
            }
            Event::End(TagEnd::Image) => {
                image_depth -= 1;
                events.push(event);
            }
            Event::Start(Tag::Link { ref dest_url, .. }) => {
                links.push(dest_url.to_string());
                events.push(event);
            }
            Event::End(TagEnd::Link) => {
                links.pop();
                events.push(event);
            }
            // pulldown-cmark reads `~x~` as strikethrough too; Markdig writes it `<sub>`.
            Event::Start(Tag::Strikethrough) => {
                let tag = if source[range].starts_with("~~") {
                    "del"
                } else {
                    "sub"
                };
                strike_tags.push(tag);
                events.push(Event::InlineHtml(format!("<{tag}>").into()));
            }
            Event::End(TagEnd::Strikethrough) => {
                let tag = strike_tags.pop().unwrap_or("del");
                events.push(Event::InlineHtml(format!("</{tag}>").into()));
            }
            Event::Start(Tag::Table(_)) => {
                headerless_table = headerless.contains(&range.start);
                events.push(event);
            }
            // A header row pulldown-cmark needs and the grid table did not have:
            // an empty head (cut from the HTML below), then the row as a body row.
            Event::Start(Tag::TableHead) if headerless_table => {
                events.push(event);
                events.push(Event::End(TagEnd::TableHead));
                events.push(Event::Start(Tag::TableRow));
            }
            Event::End(TagEnd::TableHead) if headerless_table => {
                events.push(Event::End(TagEnd::TableRow));
            }
            Event::Text(text) if code_depth == 0 && image_depth == 0 => {
                let link = links.last().map(String::as_str);
                push_text(&mut events, &text, &source, range, link);
            }
            event => events.push(event),
        }
    }
    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    out.replace("<thead><tr></tr></thead>", "")
}

/// A text event split into its plain text and its extension spans, each span
/// wrapped in the element Markdig writes for it.
fn push_text<'a>(
    events: &mut Vec<Event<'a>>,
    text: &str,
    source: &str,
    range: std::ops::Range<usize>,
    link: Option<&str>,
) {
    for piece in extensions::pieces(text, source, range, MarkdownRunStyle::default(), link) {
        let text: CowStr<'a> = piece.text.into();
        match piece.extension {
            None => events.push(Event::Text(text)),
            Some(extension) => {
                let open = match (extension.tag, extension.link.as_deref()) {
                    ("a", Some(url)) => format!("<a href=\"{}\">", escape_attribute(url)),
                    (tag, _) => format!("<{tag}>"),
                };
                events.push(Event::InlineHtml(open.into()));
                events.push(Event::Text(text));
                events.push(Event::InlineHtml(format!("</{}>", extension.tag).into()));
            }
        }
    }
}

fn escape_attribute(value: &str) -> String {
    let mut out = String::new();
    html::push_html(&mut out, std::iter::once(Event::Text(value.into())));
    out.replace('"', "&quot;")
}

/// `html` with `<b>`, `<i>`, `<s>` and `<strike>` as `<strong>`, `<em>` and
/// `<del>`, as `RichMarkdownWriterTests.Html()` normalises it.
fn normalise_style_tags(html: &str) -> String {
    let mut out = html.to_string();
    for (from, to) in [
        ("b", "strong"),
        ("i", "em"),
        ("strike", "del"),
        ("s", "del"),
    ] {
        out = out
            .replace(&format!("<{from}>"), &format!("<{to}>"))
            .replace(&format!("</{from}>"), &format!("</{to}>"));
    }
    out
}

/// `html` with every whitespace run as one space, trimmed.
fn collapse_whitespace(out: &str) -> String {
    let mut collapsed = String::with_capacity(out.len());
    let mut in_space = false;
    for c in out.chars() {
        if c.is_whitespace() {
            if !in_space {
                collapsed.push(' ');
            }
            in_space = true;
        } else {
            collapsed.push(c);
            in_space = false;
        }
    }
    collapsed.trim().to_string()
}
