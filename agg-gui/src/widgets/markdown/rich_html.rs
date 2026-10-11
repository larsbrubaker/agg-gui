//! The HTML of a `MarkdownView` selection, before styling: a port of
//! MarkdigAgg's `MarkdownWidget.BuildSelectionHtml` / `RenderSelectionNode`
//! (`MarkdigAgg/MarkdownWidget.cs`).
//!
//! C# walks the widget tree it built and renders every top-level block the
//! selection touches: a heading as `<hN>`, a paragraph as `<p>`, a list as
//! `<ul>`/`<ol>` of `<li>`, a quote as `<blockquote>`, a code block as
//! `<pre><code>`, a table whole as `<table>` with `<thead>`/`<tbody>`; within
//! a block only the selected words, with `<strong>` (not in a heading or a
//! header cell), `<em>`, `<del>`, `<code>` and `<a href>`.  The view draws
//! its layout itself, so this walks the selectable fragments
//! (`selection.rs`), grouped into blocks by their `FragmentPlace`.  The
//! result goes through `style_html_fragment` (`styled_html.rs`).
//!
//! Fixed from C#: an image is not a text widget there, so a partial
//! selection's HTML dropped every image (only a select-all, which copies the
//! whole document's HTML, kept them).  Here a selected image copies as
//! `<img src alt>` in a paragraph or a table cell
//! (`rust_only_partial_copy_keeps_images`).

use std::ops::Range;

use super::cell_images::images_of;
use super::selection::{escape_attr, escape_html, SelectableFragment, SelectableKind};
use super::{emphasis, LayoutItem, LineStyle, MarkdownRunStyle, MarkdownView};

/// The unstyled HTML of `range` of the view's plain text.
pub(super) fn selection_html(view: &MarkdownView, range: Range<usize>) -> String {
    let fragments: Vec<&SelectableFragment> = view
        .selectable_fragments
        .iter()
        .filter(|f| range.end > f.range.start && range.start < f.range.end)
        .collect();
    if fragments.is_empty() {
        // C#: a selection of separators only copies as one paragraph.
        let text = view.selectable_text.get(range).unwrap_or_default();
        return if text.is_empty() {
            String::new()
        } else {
            format!("<p>{}</p>", escape_html(text))
        };
    }

    // Consecutive fragments of one block.
    let mut blocks: Vec<Vec<&SelectableFragment>> = Vec::new();
    for fragment in fragments {
        match blocks.last_mut() {
            Some(block) if block[0].place.block == fragment.place.block => block.push(fragment),
            _ => blocks.push(vec![fragment]),
        }
    }

    let mut html = String::new();
    let mut quote_open = false;
    let mut list_open: Option<&'static str> = None;
    for block in &blocks {
        let first = block[0];
        let list = list_tag(first);
        if let Some(open) = list_open.filter(|open| list != Some(*open)) {
            html.push_str(&format!("</{open}>"));
            list_open = None;
        }
        if quote_open != first.place.quote {
            html.push_str(if quote_open {
                "</blockquote>"
            } else {
                "<blockquote>"
            });
            quote_open = first.place.quote;
        }
        if let Some(tag) = list.filter(|_| list_open.is_none()) {
            html.push_str(&format!("<{tag}>"));
            list_open = Some(tag);
        }
        html.push_str(&render_block(view, block, &range));
    }
    if let Some(tag) = list_open {
        html.push_str(&format!("</{tag}>"));
    }
    if quote_open {
        html.push_str("</blockquote>");
    }
    html
}

/// One block's HTML.
fn render_block(
    view: &MarkdownView,
    block: &[&SelectableFragment],
    range: &Range<usize>,
) -> String {
    let first = block[0];
    if let Some(table) = first.place.table {
        return view.items.get(table).map(render_table).unwrap_or_default();
    }
    if matches!(first.kind, SelectableKind::Rule) {
        // C#: a rule copies only when the selection runs across it.
        return "<hr />".to_string();
    }
    if matches!(first.kind, SelectableKind::CodeBlock) {
        let lines: Vec<String> = block
            .iter()
            .map(|f| escape_html(selected_part(f, range)))
            .filter(|line| !line.is_empty())
            .collect();
        return wrap_if_not_empty("pre", &wrap_if_not_empty("code", &lines.join("\n")));
    }

    let style = match &first.kind {
        SelectableKind::Text { style, .. } => *style,
        _ => LineStyle::Body,
    };
    let marker_len = marker_len(first);
    let mut inner = String::new();
    for (i, fragment) in block.iter().enumerate() {
        if i > 0 {
            // C# RenderSeparatorBetweenChildren: the selected separator.
            let gap_start = block[i - 1].range.end.max(range.start);
            let gap_end = fragment.range.start.min(range.end);
            if gap_start < gap_end {
                inner.push_str(&escape_html(&view.selectable_text[gap_start..gap_end]));
            }
        }
        let start = range.start.max(fragment.range.start) - fragment.range.start;
        let end = range.end.min(fragment.range.end) - fragment.range.start;
        // A list item's marker ("• ", "1. ") is the list's, not the item's text.
        let start = if i == 0 { start.max(marker_len) } else { start };
        if start < end {
            push_inline(fragment, &fragment.text[start..end], style, &mut inner);
        }
    }
    let tag = match style {
        LineStyle::H1 => "h1",
        LineStyle::H2 => "h2",
        LineStyle::H3 => "h3",
        LineStyle::H4 => "h4",
        _ if marker_len > 0 => "li",
        _ => "p",
    };
    wrap_if_not_empty(tag, &inner)
}

/// One fragment's selected text as inline HTML.
fn push_inline(fragment: &SelectableFragment, selected: &str, style: LineStyle, out: &mut String) {
    match &fragment.kind {
        SelectableKind::Text { link, code, .. } => {
            let heading = matches!(
                style,
                LineStyle::H1 | LineStyle::H2 | LineStyle::H3 | LineStyle::H4
            );
            push_wrapped(selected, out, |inner, out| {
                let mut html = if *code {
                    format!("<code>{}</code>", escape_html(inner))
                } else {
                    emphasized(escape_html(inner), fragment.run_style, !heading)
                };
                if let Some(url) = link {
                    html = format!("<a href=\"{}\">{html}</a>", escape_attr(url));
                }
                out.push_str(&html);
            });
        }
        SelectableKind::CodeBlock | SelectableKind::TableCell => {
            out.push_str(&escape_html(selected));
        }
        SelectableKind::Rule => {}
        SelectableKind::Image { url, alt, .. } => push_img(url, alt, out),
    }
}

/// C# `RenderSelectedText` + `EmphasisInlineX.WrapHtml`: struck text in
/// `<del>`, bold in `<strong>` (where `strong_allowed`), italic in `<em>`.
fn emphasized(mut html: String, run: MarkdownRunStyle, strong_allowed: bool) -> String {
    if html.is_empty() {
        return html;
    }
    if run.strikethrough {
        html = format!("<del>{html}</del>");
    }
    if run.bold && strong_allowed {
        html = format!("<strong>{html}</strong>");
    }
    if run.italic {
        html = format!("<em>{html}</em>");
    }
    html
}

/// C# `RenderTable`: every row of a table the selection touches, header
/// rows in `<thead>` as `<th>`, the rest in `<tbody>` as `<td>`.
fn render_table(item: &LayoutItem) -> String {
    let LayoutItem::Table {
        rows,
        cell_styles,
        cell_images,
        header,
        ..
    } = item
    else {
        return String::new();
    };
    if rows.is_empty() {
        return String::new();
    }
    let render_row = |ri: usize, row: &[String], tag: &str| {
        let mut html = String::from("<tr>");
        for (ci, text) in row.iter().enumerate() {
            let spans = emphasis::cell_spans(cell_styles, ri, ci);
            let images = images_of(cell_images, ri, ci);
            html.push_str(&format!("<{tag}>"));
            let mut cuts: Vec<usize> = images.iter().map(|i| i.image.at).collect();
            cuts.push(text.len());
            let mut pos = 0;
            for (k, cut) in cuts.into_iter().enumerate() {
                let cut = cut.min(text.len());
                for (piece, style) in emphasis::cell_pieces(text, spans) {
                    let start = piece.start.max(pos);
                    let end = piece.end.min(cut);
                    if start < end {
                        let text = escape_html(&text[start..end]);
                        html.push_str(&emphasized(text, style, tag == "td"));
                    }
                }
                pos = pos.max(cut);
                if let Some(image) = images.get(k) {
                    push_img(&image.image.url, &image.image.alt, &mut html);
                }
            }
            html.push_str(&format!("</{tag}>"));
        }
        html.push_str("</tr>");
        html
    };
    let header_rows = usize::from(*header);
    let mut html = String::from("<table>");
    if header_rows > 0 {
        html.push_str("<thead>");
        html.push_str(&render_row(0, &rows[0], "th"));
        html.push_str("</thead>");
    }
    if rows.len() > header_rows {
        html.push_str("<tbody>");
        for (ri, row) in rows.iter().enumerate().skip(header_rows) {
            html.push_str(&render_row(ri, row, "td"));
        }
        html.push_str("</tbody>");
    }
    html.push_str("</table>");
    html
}

fn push_img(url: &str, alt: &str, out: &mut String) {
    out.push_str("<img src=\"");
    out.push_str(&escape_attr(url));
    out.push_str("\" alt=\"");
    out.push_str(&escape_attr(alt));
    out.push_str("\" />");
}

/// The part of a fragment's text inside `range`.
fn selected_part<'a>(fragment: &'a SelectableFragment, range: &Range<usize>) -> &'a str {
    let start = range.start.max(fragment.range.start) - fragment.range.start;
    let end = range.end.min(fragment.range.end) - fragment.range.start;
    fragment.text.get(start..end.max(start)).unwrap_or_default()
}

/// C# `WrapIfNotEmpty`.
fn wrap_if_not_empty(tag: &str, inner: &str) -> String {
    if inner.is_empty() {
        String::new()
    } else {
        format!("<{tag}>{inner}</{tag}>")
    }
}

/// The list a block's first fragment makes it an item of (C#
/// `IsOrderedList` reads the item's marker), if any.
fn list_tag(fragment: &SelectableFragment) -> Option<&'static str> {
    if fragment.place.table.is_some() || !matches!(fragment.kind, SelectableKind::Text { .. }) {
        return None;
    }
    let text = &fragment.text;
    if text.starts_with("• ") {
        return Some("ul");
    }
    let dot = text.find(". ")?;
    (dot > 0 && text[..dot].chars().all(|ch| ch.is_ascii_digit())).then_some("ol")
}

/// The byte length of a list item's marker at the start of its first
/// fragment, 0 for a block that is not a list item.
fn marker_len(fragment: &SelectableFragment) -> usize {
    match list_tag(fragment) {
        Some("ul") => "• ".len(),
        Some(_) => fragment.text.find(". ").map_or(0, |dot| dot + 2),
        None => 0,
    }
}

/// Wrap the non-blank middle of `selected`, keeping its edge whitespace
/// outside the markup.
fn push_wrapped(selected: &str, out: &mut String, wrap: impl FnOnce(&str, &mut String)) {
    let content_start = selected.len() - selected.trim_start().len();
    let content_end = selected.trim_end().len().max(content_start);
    out.push_str(&escape_html(&selected[..content_start]));
    if content_start < content_end {
        wrap(&selected[content_start..content_end], out);
    }
    out.push_str(&escape_html(&selected[content_end..]));
}
