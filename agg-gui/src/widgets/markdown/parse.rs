//! Markdown event parsing for `MarkdownView`.
//!
//! This module owns the translation from pulldown-cmark events into the small
//! intermediate model consumed by the layout pass in the parent widget module.
//! Emphasis and paired HTML style tags set the style of the text between
//! them (`emphasis.rs`), so text is flushed into a new item at each change.
//! Markdig's extensions pulldown-cmark leaves as text (`^sup^`, bare links)
//! are split out of each text event by `extensions.rs`.  Pipe tables are the
//! ones Markdig finds (`markdown_editing::pipe_tables`
//! rewrites the source before pulldown-cmark reads it), and grid tables are
//! read as Markdig reads them (`grid_tables.rs`, rewriting them as pipe tables).
//! An image in a table cell stays in its cell, at its place in the cell's
//! text (`cell_images.rs`), as in MarkdigAgg's `AggTableCell`.

use pulldown_cmark::{Event as MdEvent, Options, Parser, Tag, TagEnd};

use super::alignment::{self, AlignStack, HtmlBlocks, HtmlFeed, MarkdownAlign};
use super::cell_images::{CellImage, CellImages};
use super::emphasis::{pair_style_tags, CellSpan, CellStyles, HtmlStyle, StyleStack};
use super::extensions;
use super::{InlineItem, LineStyle, MarkdownRunStyle, MarkdownView, ParagraphItem};

impl MarkdownView {
    pub(super) fn parse_paragraphs(&self) -> Vec<ParagraphItem> {
        parse_markdown(&self.markdown)
    }
}

/// The pulldown-cmark options the viewer reads with.
pub(super) const OPTIONS: Options = Options::ENABLE_STRIKETHROUGH
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_TABLES);

/// The source pulldown-cmark reads for `markdown`, with pipe tables as
/// Markdig finds them and grid tables as Markdig reads them (as pipe
/// tables), so a file reads as the same tables it did in MarkdigAgg; and the
/// byte offsets of its tables that have no header row.
pub(super) fn viewer_source(markdown: &str) -> (String, Vec<usize>) {
    let markdown = crate::widgets::markdown_editing::pipe_tables::markdig_source(markdown);
    let grid = super::grid_tables::markdig_grid_source(&markdown, OPTIONS);
    (grid.source.into_owned(), grid.headerless)
}

pub(super) fn parse_markdown(markdown: &str) -> Vec<ParagraphItem> {
    let mut out = Vec::new();
    let (source, headerless) = viewer_source(markdown);
    let (events, event_ranges): (Vec<MdEvent<'_>>, Vec<std::ops::Range<usize>>) =
        Parser::new_ext(&source, OPTIONS).into_offset_iter().unzip();
    let paired_tags = pair_style_tags(&events);
    let mut styles = StyleStack::default();
    // Whether the text flushed last ended in whitespace (see `glue`).
    let mut prev_trail = true;
    let mut table_styles: Option<CellStyles> = None;
    let mut row_styles: Option<Vec<Vec<CellSpan>>> = None;
    let mut cell_spans: Vec<CellSpan> = Vec::new();
    let mut table_images: Option<CellImages> = None;
    let mut row_images: Option<Vec<Vec<CellImage>>> = None;
    let mut cell_images: Vec<CellImage> = Vec::new();

    let mut cur_items = Vec::new();
    let mut cur_text = String::new();
    let mut cur_style = LineStyle::Body;
    let mut cur_indent = 0.0_f64;
    let mut list_depth = 0u32;
    let mut list_ordinal: Vec<Option<u64>> = Vec::new();
    let mut in_image: Option<String> = None;
    let mut link_stack: Vec<String> = Vec::new();
    let mut quote_depth = 0u32;
    let mut table: Option<Vec<Vec<String>>> = None;
    let mut table_header = true;
    let mut table_row: Option<Vec<String>> = None;
    let mut table_cell: Option<String> = None;
    let mut code_block: Option<String> = None;
    // `<div align>` blocks open around the flows emitted (`alignment.rs`); flows
    // before `stamped` already carry their alignment.
    let mut aligns = AlignStack::default();
    let mut html_blocks = HtmlBlocks::default();
    let mut stamped = 0usize;

    /// Push the pending text as one item of `style`. An item glues onto the
    /// one before it when no whitespace separates them in the source.
    fn flush_text(
        items: &mut Vec<InlineItem>,
        text: &mut String,
        link: Option<String>,
        style: MarkdownRunStyle,
        prev_trail: &mut bool,
    ) {
        let t = text.trim().to_string();
        if !t.is_empty() {
            let lead = text.starts_with(char::is_whitespace);
            items.push(InlineItem::Text {
                text: t,
                link,
                code: false,
                style,
                glue: !items.is_empty() && !*prev_trail && !lead,
            });
            *prev_trail = text.ends_with(char::is_whitespace);
        } else if !text.is_empty() {
            *prev_trail = true;
        }
        text.clear();
    }

    fn flush_code(
        items: &mut Vec<InlineItem>,
        text: &mut String,
        value: &str,
        link: Option<String>,
        style: MarkdownRunStyle,
        prev_trail: &mut bool,
    ) {
        flush_text(items, text, link.clone(), style, prev_trail);
        items.push(InlineItem::Text {
            text: value.to_string(),
            link,
            code: true,
            style,
            glue: !items.is_empty() && !*prev_trail,
        });
        *prev_trail = false;
    }

    #[allow(clippy::too_many_arguments)]
    fn flush_flow(
        out: &mut Vec<ParagraphItem>,
        items: &mut Vec<InlineItem>,
        text: &mut String,
        run_style: MarkdownRunStyle,
        prev_trail: &mut bool,
        style: LineStyle,
        indent: f64,
        quote: bool,
        link: Option<String>,
    ) {
        flush_text(items, text, link, run_style, prev_trail);
        *prev_trail = true;
        if !items.is_empty() {
            out.push(ParagraphItem::Flow {
                items: std::mem::take(items),
                style,
                indent,
                quote,
                align: MarkdownAlign::Left,
                level: 0,
            });
        }
    }

    fn add_spacer(out: &mut Vec<ParagraphItem>) {
        if !matches!(out.last(), Some(ParagraphItem::Spacer)) {
            out.push(ParagraphItem::Spacer);
        }
    }

    fn append_text(text: &mut String, value: &str) {
        // Leading whitespace is kept as one space, so `flush_text` can tell
        // `**a** b` (spaced) from `**a**b` (glued).
        if text.is_empty() && value.starts_with(char::is_whitespace) {
            text.push(' ');
            return text.push_str(value.trim_start());
        }
        // pulldown-cmark splits one run of text into several events (at an escape such as
        // `a\|b`, at an entity), so the pieces join as written; only whitespace already
        // pushed collapses with the next piece's.
        if text.ends_with(char::is_whitespace) {
            text.push_str(value.trim_start());
        } else {
            text.push_str(value);
        }
    }

    for (idx, ev) in events.into_iter().enumerate() {
        if let Some(code) = code_block.as_mut() {
            match &ev {
                MdEvent::Text(t) => {
                    code.push_str(t);
                    continue;
                }
                MdEvent::SoftBreak | MdEvent::HardBreak => {
                    code.push('\n');
                    continue;
                }
                _ => {}
            }
        }

        if let Some(cell) = table_cell.as_mut().filter(|_| in_image.is_none()) {
            // Each piece of the event with its style: an extension span (`==x==`) is bold.
            let segments: Vec<(String, MarkdownRunStyle)> = match &ev {
                MdEvent::Text(t) => {
                    extensions::pieces(t, &source, event_ranges[idx].clone(), styles.style(), None)
                        .into_iter()
                        .map(|piece| {
                            let style = piece
                                .extension
                                .map_or(styles.style(), |extension| extension.style);
                            (piece.text, style)
                        })
                        .collect()
                }
                MdEvent::Code(t) => vec![(t.to_string(), styles.style())],
                MdEvent::SoftBreak | MdEvent::HardBreak => {
                    cell.push(' ');
                    Vec::new()
                }
                _ => Vec::new(),
            };
            for (text, style) in segments {
                let before = cell.len();
                append_text(cell, &text);
                if cell.len() > before && style != MarkdownRunStyle::default() {
                    let start = before + (cell.len() - before - cell[before..].trim_start().len());
                    cell_spans.push(CellSpan {
                        start,
                        end: cell.len(),
                        style,
                    });
                }
            }
            if matches!(
                ev,
                MdEvent::Text(_) | MdEvent::Code(_) | MdEvent::SoftBreak | MdEvent::HardBreak
            ) {
                continue;
            }
        }

        let quote = quote_depth > 0;
        let link = link_stack.last().cloned();
        alignment::stamp(&mut out[stamped..], aligns.current());
        stamped = out.len();
        match html_blocks.feed(&ev) {
            HtmlFeed::Pass => {}
            HtmlFeed::Consumed => continue,
            HtmlFeed::Block(html) => {
                alignment::write_html_block(&html, &mut aligns, &mut out, cur_indent, quote);
                stamped = out.len();
                continue;
            }
        }
        if let Some((kind, open)) = emphasis_change(&ev, idx, &paired_tags) {
            if table_cell.is_none() && in_image.is_none() {
                flush_text(
                    &mut cur_items,
                    &mut cur_text,
                    link,
                    styles.style(),
                    &mut prev_trail,
                );
            }
            styles.apply(kind, open);
            continue;
        }
        match ev {
            MdEvent::Start(Tag::Image { dest_url, .. }) => {
                flush_text(
                    &mut cur_items,
                    &mut cur_text,
                    link.clone(),
                    styles.style(),
                    &mut prev_trail,
                );
                in_image = Some(dest_url.to_string());
            }
            MdEvent::End(TagEnd::Image) => {
                if let Some(url) = in_image.take() {
                    let alt = cur_text.trim().to_string();
                    cur_text.clear();
                    if let Some(cell) = table_cell.as_ref() {
                        // A cell keeps its images where they sit in its text.
                        cell_images.push(CellImage {
                            at: cell.len(),
                            url,
                            alt,
                            link: link.clone(),
                        });
                    } else {
                        cur_items.push(InlineItem::Image {
                            url,
                            alt,
                            link: link.clone(),
                        });
                        prev_trail = true;
                    }
                }
            }
            MdEvent::Text(t) if in_image.is_some() => cur_text.push_str(&t),
            MdEvent::Start(Tag::Link { dest_url, .. }) => {
                flush_text(
                    &mut cur_items,
                    &mut cur_text,
                    link.clone(),
                    styles.style(),
                    &mut prev_trail,
                );
                link_stack.push(dest_url.to_string());
            }
            MdEvent::End(TagEnd::Link) => {
                flush_text(
                    &mut cur_items,
                    &mut cur_text,
                    link.clone(),
                    styles.style(),
                    &mut prev_trail,
                );
                link_stack.pop();
            }
            MdEvent::Start(Tag::BlockQuote(_)) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
                    styles.style(),
                    &mut prev_trail,
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                quote_depth += 1;
                cur_indent += 16.0;
            }
            MdEvent::End(TagEnd::BlockQuote) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
                    styles.style(),
                    &mut prev_trail,
                    cur_style,
                    cur_indent,
                    true,
                    link,
                );
                quote_depth = quote_depth.saturating_sub(1);
                cur_indent = (cur_indent - 16.0).max(0.0);
                add_spacer(&mut out);
            }
            MdEvent::Start(Tag::Table(_)) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
                    styles.style(),
                    &mut prev_trail,
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                table = Some(Vec::new());
                table_styles = Some(Vec::new());
                table_images = Some(Vec::new());
                table_header = !headerless.contains(&event_ranges[idx].start);
            }
            MdEvent::End(TagEnd::Table) => {
                if let Some(rows) = table.take() {
                    out.push(ParagraphItem::Table {
                        rows,
                        cell_styles: table_styles.take().unwrap_or_default(),
                        cell_images: table_images.take().unwrap_or_default(),
                        header: table_header,
                    });
                    add_spacer(&mut out);
                }
            }
            // pulldown-cmark wraps the header cells in a TableHead rather
            // than a TableRow, so both open and close a row.
            MdEvent::Start(Tag::TableRow | Tag::TableHead) => {
                table_row = Some(Vec::new());
                row_styles = Some(Vec::new());
                row_images = Some(Vec::new());
            }
            MdEvent::End(TagEnd::TableRow | TagEnd::TableHead) => {
                if let (Some(rows), Some(row)) = (table.as_mut(), table_row.take()) {
                    rows.push(row);
                    if let Some(all) = table_styles.as_mut() {
                        all.push(row_styles.take().unwrap_or_default());
                    }
                    if let Some(all) = table_images.as_mut() {
                        all.push(row_images.take().unwrap_or_default());
                    }
                }
            }
            MdEvent::Start(Tag::TableCell) => {
                table_cell = Some(String::new());
                cell_spans.clear();
                cell_images.clear();
            }
            MdEvent::End(TagEnd::TableCell) => {
                if let (Some(row), Some(cell)) = (table_row.as_mut(), table_cell.take()) {
                    let cut = cell.len() - cell.trim_start().len();
                    let trimmed = cell.trim().to_string();
                    let spans = std::mem::take(&mut cell_spans)
                        .into_iter()
                        .filter_map(|span| {
                            let start = span.start.saturating_sub(cut).min(trimmed.len());
                            let end = span.end.saturating_sub(cut).min(trimmed.len());
                            (end > start).then_some(CellSpan { start, end, ..span })
                        })
                        .collect();
                    let images = std::mem::take(&mut cell_images)
                        .into_iter()
                        .map(|image| CellImage {
                            at: image.at.saturating_sub(cut).min(trimmed.len()),
                            ..image
                        })
                        .collect();
                    row.push(trimmed);
                    if let Some(styles_row) = row_styles.as_mut() {
                        styles_row.push(spans);
                    }
                    if let Some(images_row) = row_images.as_mut() {
                        images_row.push(images);
                    }
                }
            }
            MdEvent::Start(Tag::Heading { level, .. }) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
                    styles.style(),
                    &mut prev_trail,
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                cur_style = match level as u8 {
                    1 => LineStyle::H1,
                    2 => LineStyle::H2,
                    3 => LineStyle::H3,
                    _ => LineStyle::H4,
                };
                cur_indent = if quote { 16.0 } else { 0.0 };
            }
            MdEvent::End(TagEnd::Heading(level)) => {
                let before = out.len();
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
                    styles.style(),
                    &mut prev_trail,
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                if let Some(ParagraphItem::Flow { level: l, .. }) = out.get_mut(before) {
                    *l = level as u8;
                }
                add_spacer(&mut out);
                cur_style = LineStyle::Body;
                cur_indent = if quote { 16.0 } else { 0.0 };
            }
            MdEvent::Start(Tag::Paragraph) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
                    styles.style(),
                    &mut prev_trail,
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
            }
            MdEvent::End(TagEnd::Paragraph) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
                    styles.style(),
                    &mut prev_trail,
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                add_spacer(&mut out);
            }
            MdEvent::Start(Tag::List(first)) => {
                list_depth += 1;
                list_ordinal.push(first);
                cur_indent = (if quote { 16.0 } else { 0.0 }) + list_depth as f64 * 16.0;
            }
            MdEvent::End(TagEnd::List(_)) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
                    styles.style(),
                    &mut prev_trail,
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                list_depth = list_depth.saturating_sub(1);
                list_ordinal.pop();
                cur_indent = (if quote { 16.0 } else { 0.0 }) + list_depth as f64 * 16.0;
                if list_depth == 0 {
                    add_spacer(&mut out);
                }
            }
            MdEvent::Start(Tag::Item) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
                    styles.style(),
                    &mut prev_trail,
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                if let Some(Some(n)) = list_ordinal.last_mut() {
                    cur_text = format!("{}. ", n);
                    *n += 1;
                } else {
                    cur_text = "• ".to_string();
                }
            }
            MdEvent::End(TagEnd::Item) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
                    styles.style(),
                    &mut prev_trail,
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
            }
            MdEvent::Start(Tag::CodeBlock(_)) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
                    styles.style(),
                    &mut prev_trail,
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                code_block = Some(String::new());
            }
            MdEvent::End(TagEnd::CodeBlock) => {
                if let Some(code) = code_block.take() {
                    let mut lines: Vec<String> =
                        code.lines().map(|line| line.to_string()).collect();
                    if lines.is_empty() {
                        lines.push(String::new());
                    }
                    out.push(ParagraphItem::CodeBlock(lines));
                }
                add_spacer(&mut out);
                cur_style = LineStyle::Body;
            }
            MdEvent::Rule => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
                    styles.style(),
                    &mut prev_trail,
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                out.push(ParagraphItem::Rule);
            }
            MdEvent::Text(t) => {
                for piece in extensions::pieces(
                    &t,
                    &source,
                    event_ranges[idx].clone(),
                    styles.style(),
                    link.as_deref(),
                ) {
                    let Some(extension) = piece.extension else {
                        append_text(&mut cur_text, &piece.text);
                        continue;
                    };
                    flush_text(
                        &mut cur_items,
                        &mut cur_text,
                        link.clone(),
                        styles.style(),
                        &mut prev_trail,
                    );
                    cur_text.push_str(&piece.text);
                    flush_text(
                        &mut cur_items,
                        &mut cur_text,
                        extension.link,
                        extension.style,
                        &mut prev_trail,
                    );
                }
            }
            MdEvent::Code(t) => flush_code(
                &mut cur_items,
                &mut cur_text,
                &t,
                link,
                styles.style(),
                &mut prev_trail,
            ),
            MdEvent::SoftBreak | MdEvent::HardBreak => cur_text.push(' '),
            _ => {}
        }
    }

    flush_flow(
        &mut out,
        &mut cur_items,
        &mut cur_text,
        styles.style(),
        &mut prev_trail,
        cur_style,
        cur_indent,
        quote_depth > 0,
        link_stack.last().cloned(),
    );
    alignment::stamp(&mut out[stamped..], aligns.current());
    out
}

/// The style an event opens or closes: an emphasis span, or an inline HTML
/// style tag that pairs within its container.
fn emphasis_change(
    ev: &MdEvent<'_>,
    idx: usize,
    paired: &std::collections::HashMap<usize, (HtmlStyle, bool)>,
) -> Option<(HtmlStyle, bool)> {
    match ev {
        MdEvent::Start(Tag::Emphasis) => Some((HtmlStyle::Italic, true)),
        MdEvent::End(TagEnd::Emphasis) => Some((HtmlStyle::Italic, false)),
        MdEvent::Start(Tag::Strong) => Some((HtmlStyle::Bold, true)),
        MdEvent::End(TagEnd::Strong) => Some((HtmlStyle::Bold, false)),
        MdEvent::Start(Tag::Strikethrough) => Some((HtmlStyle::Strike, true)),
        MdEvent::End(TagEnd::Strikethrough) => Some((HtmlStyle::Strike, false)),
        MdEvent::InlineHtml(_) => paired.get(&idx).copied(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fenced_code_blocks_preserve_lines_and_indentation() {
        let items = parse_markdown("```toml\n[dependencies]\nagg-gui = \"0.1\"\n  indented\n```");
        let code = items.iter().find_map(|item| {
            if let ParagraphItem::CodeBlock(lines) = item {
                Some(lines)
            } else {
                None
            }
        });

        assert_eq!(
            code,
            Some(&vec![
                "[dependencies]".to_string(),
                "agg-gui = \"0.1\"".to_string(),
                "  indented".to_string()
            ])
        );
    }

    #[test]
    fn unordered_lists_preserve_bullet_markers() {
        let items = parse_markdown("- Alpha\n- Beta");
        let first_text = items.iter().find_map(|item| {
            if let ParagraphItem::Flow { items, .. } = item {
                items.first().and_then(|inline| {
                    if let InlineItem::Text { text, .. } = inline {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        });

        assert_eq!(first_text, Some("• Alpha"));
    }

    fn tables(items: &[ParagraphItem]) -> Vec<Vec<Vec<String>>> {
        items
            .iter()
            .filter_map(|item| match item {
                ParagraphItem::Table { rows, .. } => Some(rows.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn rust_only_pipe_tables_are_the_ones_markdig_finds() {
        // A pipe in a code span keeps its cell (GFM would split it and refuse the table).
        let items = parse_markdown("| a\\|b | `c|d` |\n|---|---|\n| x | `y|z` |");
        assert_eq!(
            tables(&items),
            [vec![
                vec!["a|b".to_string(), "c|d".to_string()],
                vec!["x".to_string(), "y|z".to_string()],
            ]]
        );

        // Header and delimiter rows of different widths, and a wider body row.
        let items = parse_markdown("| a |\n|---|---|\n\n| a | b |\n|---|\n| c | d | e |");
        assert_eq!(
            tables(&items),
            [
                vec![vec!["a".to_string()]],
                vec![
                    vec!["a".to_string(), "b".to_string(), String::new()],
                    vec!["c".to_string(), "d".to_string(), "e".to_string()],
                ],
            ]
        );

        // A line without a pipe after the header makes the whole paragraph text.
        let items = parse_markdown("| a | b |\n|---|---|\nplain");
        assert!(tables(&items).is_empty());
        let ParagraphItem::Flow { items: inline, .. } = &items[0] else {
            panic!("expected a paragraph");
        };
        let InlineItem::Text { text, .. } = &inline[0] else {
            panic!("expected text");
        };
        assert_eq!(text, "| a | b | |---|---| plain");
    }
}
