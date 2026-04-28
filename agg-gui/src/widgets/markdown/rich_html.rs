//! Rich HTML clipboard rendering for `MarkdownView` selections.
//!
//! Clipboard HTML is consumed by applications with their own user-agent styles.
//! This module emits inline styles that mirror the Markdown renderer more
//! closely than default `<h1>` / plaintext table markup.

use std::ops::Range;

use super::selection::{escape_attr, escape_html, SelectableFragment, SelectableKind};
use super::LineStyle;

pub(super) fn copy_html(
    fragments: &[SelectableFragment],
    selectable_text: &str,
    range: Range<usize>,
) -> String {
    let mut out = String::new();
    let mut cursor = range.start;
    let mut idx = 0usize;

    while idx < fragments.len() {
        let fragment = &fragments[idx];
        if fragment.range.end <= range.start {
            idx += 1;
            continue;
        }
        if fragment.range.start >= range.end {
            break;
        }

        let gap_end = fragment.range.start.min(range.end);
        if cursor < gap_end {
            out.push_str(&html_gap(&selectable_text[cursor..gap_end]));
        }

        if matches!(fragment.kind, SelectableKind::TableCell) {
            let (next_idx, next_cursor, table) =
                collect_table(fragments, selectable_text, range.clone(), idx);
            out.push_str(&table);
            idx = next_idx;
            cursor = next_cursor;
            continue;
        }
        if let Some(marker) = list_marker(fragment, range.clone()) {
            let (next_idx, next_cursor, list) =
                collect_list(fragments, selectable_text, range.clone(), idx, marker);
            out.push_str(&list);
            idx = next_idx;
            cursor = next_cursor;
            continue;
        }

        let start = range.start.max(fragment.range.start);
        let end = range.end.min(fragment.range.end);
        if start < end {
            let selected = &fragment.text[start - fragment.range.start..end - fragment.range.start];
            push_fragment_html(fragment, selected, &mut out);
        }
        cursor = end;
        idx += 1;
    }

    if cursor < range.end {
        out.push_str(&html_gap(&selectable_text[cursor..range.end]));
    }

    out
}

fn collect_table(
    fragments: &[SelectableFragment],
    selectable_text: &str,
    range: Range<usize>,
    start_idx: usize,
) -> (usize, usize, String) {
    let mut rows: Vec<Vec<String>> = vec![Vec::new()];
    let mut idx = start_idx;
    let mut cursor = range.start.max(fragments[start_idx].range.start);
    let mut last_end = cursor;

    while let Some(fragment) = fragments.get(idx) {
        if fragment.range.start >= range.end {
            break;
        }
        if !matches!(fragment.kind, SelectableKind::TableCell) {
            break;
        }

        let gap_start = last_end.min(selectable_text.len());
        let gap_end = fragment.range.start.min(range.end);
        if gap_start < gap_end && selectable_text[gap_start..gap_end].contains('\n') {
            rows.push(Vec::new());
        }

        let start = range.start.max(fragment.range.start);
        let end = range.end.min(fragment.range.end);
        if start < end {
            let selected = &fragment.text[start - fragment.range.start..end - fragment.range.start];
            rows.last_mut()
                .expect("table always has a current row")
                .push(escape_html(selected));
            cursor = end;
            last_end = end;
        }

        idx += 1;
        if let Some(next) = fragments.get(idx) {
            let gap = &selectable_text
                [last_end.min(selectable_text.len())..next.range.start.min(selectable_text.len())];
            if !matches!(next.kind, SelectableKind::TableCell)
                || gap.chars().any(|ch| ch != '\t' && ch != '\n')
            {
                break;
            }
        }
    }

    rows.retain(|row| !row.is_empty());
    let mut html = String::new();
    html.push_str("<table style=\"border-collapse:collapse;font-size:");
    html.push_str(&format_px(fragments[start_idx].font_size));
    html.push_str(";margin:4px 0;\">");
    for (row_idx, row) in rows.iter().enumerate() {
        html.push_str("<tr>");
        let tag = if row_idx == 0 { "th" } else { "td" };
        for cell in row {
            html.push('<');
            html.push_str(tag);
            html.push_str(" style=\"border:1px solid #cfcfcf;padding:4px 8px;text-align:left;");
            if row_idx == 0 {
                html.push_str("background:#eeeeee;font-weight:600;");
            } else {
                html.push_str("font-weight:400;");
            }
            html.push_str("\">");
            html.push_str(cell);
            html.push_str("</");
            html.push_str(tag);
            html.push('>');
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");

    (idx, cursor, html)
}

fn collect_list(
    fragments: &[SelectableFragment],
    selectable_text: &str,
    range: Range<usize>,
    start_idx: usize,
    first_marker: ListMarker,
) -> (usize, usize, String) {
    let mut idx = start_idx;
    let mut cursor = range.start.max(fragments[start_idx].range.start);
    let mut html = String::new();

    match first_marker.kind {
        ListKind::Ordered { start } => {
            if start == 1 {
                html.push_str("<ol");
            } else {
                html.push_str("<ol start=\"");
                html.push_str(&start.to_string());
                html.push('"');
            }
        }
        ListKind::Bullet => html.push_str("<ul"),
    }
    html.push_str(" style=\"font-size:");
    html.push_str(&format_px(fragments[start_idx].font_size));
    html.push_str(";margin:4px 0 4px 22px;padding-left:18px;\">");

    while let Some(fragment) = fragments.get(idx) {
        if fragment.range.start >= range.end {
            break;
        }
        let Some(marker) = list_marker(fragment, range.clone()) else {
            break;
        };
        if marker.kind.variant_id() != first_marker.kind.variant_id() {
            break;
        }

        html.push_str("<li style=\"margin:2px 0;\">");
        let (next_idx, next_cursor, item_html) =
            collect_list_item(fragments, selectable_text, range.clone(), idx, marker);
        html.push_str(&item_html);
        html.push_str("</li>");

        idx = next_idx;
        cursor = next_cursor;

        let Some(next) = fragments.get(idx) else {
            break;
        };
        let gap = &selectable_text
            [cursor.min(selectable_text.len())..next.range.start.min(selectable_text.len())];
        let Some(next_marker) = list_marker(next, range.clone()) else {
            break;
        };
        if next_marker.kind.variant_id() != first_marker.kind.variant_id()
            || gap.chars().any(|ch| ch != '\n' && ch != ' ' && ch != '\t')
        {
            break;
        }
    }

    html.push_str(match first_marker.kind {
        ListKind::Ordered { .. } => "</ol>",
        ListKind::Bullet => "</ul>",
    });
    (idx, cursor, html)
}

fn collect_list_item(
    fragments: &[SelectableFragment],
    selectable_text: &str,
    range: Range<usize>,
    start_idx: usize,
    marker: ListMarker,
) -> (usize, usize, String) {
    let mut html = String::new();
    let mut idx = start_idx;
    let mut cursor = range.start.max(fragments[start_idx].range.start);
    let mut last_end = cursor;

    while let Some(fragment) = fragments.get(idx) {
        if fragment.range.start >= range.end {
            break;
        }
        if idx != start_idx && list_marker(fragment, range.clone()).is_some() {
            break;
        }
        if idx != start_idx {
            let gap = &selectable_text[last_end.min(selectable_text.len())
                ..fragment.range.start.min(selectable_text.len())];
            if gap.contains('\n') {
                break;
            }
            html.push_str(&html_gap(gap));
        }

        let start = range.start.max(fragment.range.start);
        let end = range.end.min(fragment.range.end);
        if start < end {
            let local_start = if idx == start_idx {
                (start - fragment.range.start).max(marker.prefix_len)
            } else {
                start - fragment.range.start
            };
            let local_end = end - fragment.range.start;
            if local_start < local_end {
                let selected = &fragment.text[local_start..local_end];
                push_fragment_html(fragment, selected, &mut html);
            }
            cursor = end;
            last_end = end;
        }

        idx += 1;
    }

    (idx, cursor, html)
}

fn push_fragment_html(fragment: &SelectableFragment, selected: &str, out: &mut String) {
    match &fragment.kind {
        SelectableKind::Text { style, link, code } => {
            if let Some(url) = link {
                push_wrapped_html(selected, out, |inner, out| {
                    out.push_str("<a href=\"");
                    out.push_str(&escape_attr(url));
                    out.push_str("\" style=\"color:#06c;text-decoration:underline;font-size:");
                    out.push_str(&format_px(fragment.font_size));
                    out.push_str(";\">");
                    out.push_str(&escape_html(inner));
                    out.push_str("</a>");
                });
            } else if *code {
                push_wrapped_html(selected, out, |inner, out| {
                    out.push_str("<code style=\"font-family:Consolas,monospace;background:#eeeeee;border-radius:3px;padding:1px 4px;font-size:");
                    out.push_str(&format_px(fragment.font_size));
                    out.push_str(";\">");
                    out.push_str(&escape_html(inner));
                    out.push_str("</code>");
                });
            } else {
                out.push_str("<span style=\"");
                push_text_style(*style, fragment.font_size, out);
                out.push_str("\">");
                out.push_str(&escape_html(selected));
                out.push_str("</span>");
            }
        }
        SelectableKind::CodeBlock => {
            out.push_str("<pre style=\"font-family:Consolas,monospace;background:#eeeeee;border-radius:4px;padding:8px;font-size:");
            out.push_str(&format_px(fragment.font_size));
            out.push_str(";white-space:pre-wrap;\"><code>");
            out.push_str(&escape_html(selected));
            out.push_str("</code></pre>");
        }
        SelectableKind::TableCell => {}
        SelectableKind::Image { url, alt, .. } => {
            out.push_str("<img src=\"");
            out.push_str(&escape_attr(url));
            out.push_str("\" alt=\"");
            out.push_str(&escape_attr(alt));
            out.push_str("\" style=\"max-width:100%;\">");
        }
    }
}

fn push_text_style(style: LineStyle, font_size: f64, out: &mut String) {
    out.push_str("font-size:");
    out.push_str(&format_px(font_size));
    out.push(';');
    if matches!(
        style,
        LineStyle::H1 | LineStyle::H2 | LineStyle::H3 | LineStyle::H4
    ) {
        out.push_str("font-weight:600;");
    } else {
        out.push_str("font-weight:400;");
    }
}

fn html_gap(gap: &str) -> String {
    escape_html(gap)
        .replace('\n', "<br>\n")
        .replace('\t', "    ")
}

fn list_marker(fragment: &SelectableFragment, range: Range<usize>) -> Option<ListMarker> {
    let start = range.start.max(fragment.range.start);
    if start != fragment.range.start || !matches!(fragment.kind, SelectableKind::Text { .. }) {
        return None;
    }
    parse_list_marker(&fragment.text)
}

fn parse_list_marker(text: &str) -> Option<ListMarker> {
    if text.starts_with("• ") {
        return Some(ListMarker {
            kind: ListKind::Bullet,
            prefix_len: "• ".len(),
        });
    }

    let dot = text.find(". ")?;
    if dot == 0 || !text[..dot].chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let start = text[..dot].parse().ok()?;
    Some(ListMarker {
        kind: ListKind::Ordered { start },
        prefix_len: dot + 2,
    })
}

#[derive(Clone, Copy)]
struct ListMarker {
    kind: ListKind,
    prefix_len: usize,
}

#[derive(Clone, Copy)]
enum ListKind {
    Ordered { start: u64 },
    Bullet,
}

impl ListKind {
    fn variant_id(self) -> u8 {
        match self {
            ListKind::Ordered { .. } => 0,
            ListKind::Bullet => 1,
        }
    }
}

fn format_px(value: f64) -> String {
    format!("{value:.1}px")
}

fn push_wrapped_html(selected: &str, out: &mut String, wrap: impl FnOnce(&str, &mut String)) {
    let leading_len = selected.len() - selected.trim_start().len();
    let trailing_len = selected.len() - selected.trim_end().len();
    let content_start = leading_len;
    let content_end = selected.len().saturating_sub(trailing_len);
    out.push_str(&escape_html(&selected[..content_start]));
    if content_start < content_end {
        wrap(&selected[content_start..content_end], out);
    }
    out.push_str(&escape_html(&selected[content_end..]));
}
