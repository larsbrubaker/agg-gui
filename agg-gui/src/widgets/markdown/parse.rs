//! Markdown event parsing for `MarkdownView`.
//!
//! This module owns the translation from pulldown-cmark events into the small
//! intermediate model consumed by the layout pass in the parent widget module.

use pulldown_cmark::{Event as MdEvent, Options, Parser, Tag, TagEnd};

use super::{InlineItem, LineStyle, MarkdownView, ParagraphItem};

impl MarkdownView {
    pub(super) fn parse_paragraphs(&self) -> Vec<ParagraphItem> {
        parse_markdown(&self.markdown)
    }
}

fn parse_markdown(markdown: &str) -> Vec<ParagraphItem> {
    let mut out = Vec::new();
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(markdown, opts);

    let mut cur_items = Vec::new();
    let mut cur_text = String::new();
    let mut cur_style = LineStyle::Body;
    let mut cur_indent = 0.0_f64;
    let mut list_depth = 0u32;
    let mut list_ordinal: Vec<u64> = Vec::new();
    let mut in_image: Option<String> = None;
    let mut link_stack: Vec<String> = Vec::new();
    let mut quote_depth = 0u32;
    let mut table: Option<Vec<Vec<String>>> = None;
    let mut table_row: Option<Vec<String>> = None;
    let mut table_cell: Option<String> = None;
    let mut code_block: Option<String> = None;

    fn flush_text(items: &mut Vec<InlineItem>, text: &mut String, link: Option<String>) {
        let t = text.trim().to_string();
        if !t.is_empty() {
            items.push(InlineItem::Text {
                text: t,
                link,
                code: false,
            });
        }
        text.clear();
    }

    fn flush_code(
        items: &mut Vec<InlineItem>,
        text: &mut String,
        value: &str,
        link: Option<String>,
    ) {
        flush_text(items, text, link.clone());
        items.push(InlineItem::Text {
            text: value.to_string(),
            link,
            code: true,
        });
    }

    fn flush_flow(
        out: &mut Vec<ParagraphItem>,
        items: &mut Vec<InlineItem>,
        text: &mut String,
        style: LineStyle,
        indent: f64,
        quote: bool,
        link: Option<String>,
    ) {
        flush_text(items, text, link);
        if !items.is_empty() {
            out.push(ParagraphItem::Flow {
                items: std::mem::take(items),
                style,
                indent,
                quote,
            });
        }
    }

    fn add_spacer(out: &mut Vec<ParagraphItem>) {
        if !matches!(out.last(), Some(ParagraphItem::Spacer)) {
            out.push(ParagraphItem::Spacer);
        }
    }

    fn append_text(text: &mut String, value: &str) {
        if !text.is_empty() && !text.ends_with(' ') && !text.ends_with('\n') {
            text.push(' ');
        }
        text.push_str(value.trim_start());
    }

    for ev in parser {
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

        if let Some(cell) = table_cell.as_mut() {
            match &ev {
                MdEvent::Text(t) => append_text(cell, t),
                MdEvent::Code(t) => append_text(cell, &t),
                MdEvent::SoftBreak | MdEvent::HardBreak => cell.push(' '),
                _ => {}
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
        match ev {
            MdEvent::Start(Tag::Image { dest_url, .. }) => {
                flush_text(&mut cur_items, &mut cur_text, link.clone());
                in_image = Some(dest_url.to_string());
            }
            MdEvent::End(TagEnd::Image) => {
                if let Some(url) = in_image.take() {
                    let alt = cur_text.trim().to_string();
                    cur_text.clear();
                    cur_items.push(InlineItem::Image {
                        url,
                        alt,
                        link: link.clone(),
                    });
                }
            }
            MdEvent::Text(t) if in_image.is_some() => cur_text.push_str(&t),
            MdEvent::Start(Tag::Link { dest_url, .. }) => {
                flush_text(&mut cur_items, &mut cur_text, link.clone());
                link_stack.push(dest_url.to_string());
            }
            MdEvent::End(TagEnd::Link) => {
                flush_text(&mut cur_items, &mut cur_text, link.clone());
                link_stack.pop();
            }
            MdEvent::Start(Tag::BlockQuote(_)) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
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
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                table = Some(Vec::new());
            }
            MdEvent::End(TagEnd::Table) => {
                if let Some(rows) = table.take() {
                    out.push(ParagraphItem::Table(rows));
                    add_spacer(&mut out);
                }
            }
            MdEvent::Start(Tag::TableRow) => table_row = Some(Vec::new()),
            MdEvent::End(TagEnd::TableRow) => {
                if let (Some(rows), Some(row)) = (table.as_mut(), table_row.take()) {
                    rows.push(row);
                }
            }
            MdEvent::Start(Tag::TableCell) => table_cell = Some(String::new()),
            MdEvent::End(TagEnd::TableCell) => {
                if let (Some(row), Some(cell)) = (table_row.as_mut(), table_cell.take()) {
                    row.push(cell.trim().to_string());
                }
            }
            MdEvent::Start(Tag::Heading { level, .. }) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
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
            MdEvent::End(TagEnd::Heading(_)) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                add_spacer(&mut out);
                cur_style = LineStyle::Body;
                cur_indent = if quote { 16.0 } else { 0.0 };
            }
            MdEvent::Start(Tag::Paragraph) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
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
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                add_spacer(&mut out);
            }
            MdEvent::Start(Tag::List(first)) => {
                list_depth += 1;
                list_ordinal.push(first.unwrap_or(1));
                cur_indent = (if quote { 16.0 } else { 0.0 }) + list_depth as f64 * 16.0;
            }
            MdEvent::End(TagEnd::List(_)) => {
                flush_flow(
                    &mut out,
                    &mut cur_items,
                    &mut cur_text,
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
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                if let Some(n) = list_ordinal.last_mut() {
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
                    cur_style,
                    cur_indent,
                    quote,
                    link,
                );
                out.push(ParagraphItem::Rule);
            }
            MdEvent::Text(t) => append_text(&mut cur_text, &t),
            MdEvent::Code(t) => flush_code(&mut cur_items, &mut cur_text, &t, link),
            MdEvent::SoftBreak | MdEvent::HardBreak => cur_text.push(' '),
            _ => {}
        }
    }

    flush_flow(
        &mut out,
        &mut cur_items,
        &mut cur_text,
        cur_style,
        cur_indent,
        quote_depth > 0,
        link_stack.last().cloned(),
    );
    out
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
}
