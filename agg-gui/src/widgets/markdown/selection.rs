//! Selection, hit-testing, highlighting, and rich-copy for `MarkdownView`.
//!
//! The renderer lays out Markdown into visual fragments. This module builds a
//! parallel selectable model from those fragments so hit-testing, highlighting,
//! and clipboard output all agree with the painted geometry.

use std::ops::Range;
use std::sync::Arc;

use crate::clipboard;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};
use crate::text::{measure_advance, Font};

use super::{LayoutItem, LineRun, LineStyle, MarkdownView, BLOCK_SCROLLBAR_GAP, BLOCK_SCROLLBAR_H};

#[derive(Clone)]
pub(super) struct SelectableFragment {
    pub range: Range<usize>,
    pub(super) text: String,
    pub(super) kind: SelectableKind,
    pub(super) text_x: f64,
    pub(super) y: f64,
    pub(super) height: f64,
    pub(super) width: f64,
    pub(super) font_size: f64,
    pub(super) clip: Option<Rect>,
    pub(super) line_prefix: Option<&'static str>,
}

#[derive(Clone)]
pub(super) enum SelectableKind {
    Text {
        style: LineStyle,
        link: Option<String>,
        code: bool,
    },
    CodeBlock,
    TableCell,
    Image {
        url: String,
        alt: String,
        cache_idx: usize,
    },
}

impl MarkdownView {
    pub(super) fn rebuild_selection_model(&mut self) {
        self.selectable_text.clear();
        self.selectable_fragments.clear();

        let pad = self.padding;
        let font = self.active_font();
        let items = self.items.clone();
        for item in &items {
            match item {
                LayoutItem::Line {
                    runs,
                    style,
                    indent,
                    y,
                    height,
                    ..
                } => {
                    if runs.is_empty() {
                        self.push_selection_gap("\n");
                        continue;
                    }

                    let fs = style.font_size(self.font_size);
                    let metrics = crate::text::measure_text_metrics(&font, "", fs);
                    let ty = y + height * 0.5;
                    let text_y = ty - (metrics.ascent - metrics.descent) * 0.5;
                    let tx = pad + indent;
                    let mut first_text = true;

                    for run in runs {
                        match run {
                            LineRun::Text {
                                text,
                                link,
                                code,
                                x,
                                width,
                            } => {
                                let text_x = if matches!(style, LineStyle::Code) {
                                    tx + x + 4.0
                                } else if *code {
                                    tx + x + self.font_size * 0.35
                                } else {
                                    tx + x
                                };
                                let prefix = if first_text {
                                    heading_prefix(*style)
                                } else {
                                    None
                                };
                                first_text = false;
                                self.push_fragment(SelectableFragment {
                                    range: 0..0,
                                    text: text.clone(),
                                    kind: SelectableKind::Text {
                                        style: *style,
                                        link: link.clone(),
                                        code: *code,
                                    },
                                    text_x,
                                    y: *y,
                                    height: *height,
                                    width: *width,
                                    font_size: fs,
                                    clip: None,
                                    line_prefix: prefix,
                                });
                                let _ = text_y;
                            }
                            LineRun::Image {
                                url,
                                alt,
                                cache_idx,
                                x,
                                y_offset,
                                width,
                                height: image_h,
                                ..
                            } => {
                                let label = if alt.is_empty() { "image" } else { alt };
                                self.push_fragment(SelectableFragment {
                                    range: 0..0,
                                    text: label.to_string(),
                                    kind: SelectableKind::Image {
                                        url: url.clone(),
                                        alt: alt.clone(),
                                        cache_idx: *cache_idx,
                                    },
                                    text_x: tx + x,
                                    y: y + y_offset,
                                    height: *image_h,
                                    width: *width,
                                    font_size: self.font_size,
                                    clip: None,
                                    line_prefix: None,
                                });
                            }
                        }
                    }
                }
                LayoutItem::CodeBlock {
                    block_idx,
                    lines,
                    y,
                    height,
                    line_h,
                    viewport_width,
                    content_width,
                } => {
                    let scroll_x = self.block_scroll_offset(*block_idx);
                    let scrollbar_h = if content_width > viewport_width {
                        BLOCK_SCROLLBAR_H + BLOCK_SCROLLBAR_GAP
                    } else {
                        0.0
                    };
                    let code_pad_x = self.font_size;
                    let code_pad_y = self.font_size * 0.75;
                    let content_y = y + scrollbar_h;
                    let content_h = height - scrollbar_h;
                    let mut line_top = content_y + content_h - code_pad_y - line_h;
                    for (idx, line) in lines.iter().enumerate() {
                        if idx > 0 {
                            self.push_selection_gap("\n");
                        }
                        self.push_fragment(SelectableFragment {
                            range: 0..0,
                            text: line.clone(),
                            kind: SelectableKind::CodeBlock,
                            text_x: pad + code_pad_x - scroll_x,
                            y: line_top,
                            height: *line_h,
                            width: measure_advance(
                                &font,
                                line,
                                LineStyle::Code.font_size(self.font_size),
                            ),
                            font_size: LineStyle::Code.font_size(self.font_size),
                            clip: Some(Rect::new(pad, content_y, *viewport_width, content_h)),
                            line_prefix: None,
                        });
                        line_top -= line_h;
                    }
                }
                LayoutItem::Table {
                    block_idx,
                    rows,
                    y,
                    row_h,
                    col_widths,
                    viewport_width,
                    content_width,
                    ..
                } => {
                    let scroll_x = self.block_scroll_offset(*block_idx);
                    let scrollbar_h = if content_width > viewport_width {
                        BLOCK_SCROLLBAR_H + BLOCK_SCROLLBAR_GAP
                    } else {
                        0.0
                    };
                    let content_y = y + scrollbar_h;
                    let clip =
                        Rect::new(pad, content_y, *viewport_width, row_h * rows.len() as f64);
                    let mut cy = content_y + row_h * rows.len() as f64;
                    for (ri, row) in rows.iter().enumerate() {
                        if ri > 0 {
                            self.push_selection_gap("\n");
                        }
                        cy -= row_h;
                        let mut cx = pad - scroll_x;
                        for (ci, width) in col_widths.iter().enumerate() {
                            if ci > 0 {
                                self.push_selection_gap("\t");
                            }
                            if let Some(text) = row.get(ci) {
                                self.push_fragment(SelectableFragment {
                                    range: 0..0,
                                    text: text.clone(),
                                    kind: SelectableKind::TableCell,
                                    text_x: cx + 8.0,
                                    y: cy,
                                    height: *row_h,
                                    width: measure_advance(&font, text, self.font_size),
                                    font_size: self.font_size,
                                    clip: Some(clip),
                                    line_prefix: None,
                                });
                            }
                            cx += width;
                        }
                    }
                }
            }
        }

        self.clamp_selection_to_text();
    }

    fn push_selection_gap(&mut self, gap: &str) {
        if !self.selectable_text.ends_with(gap) {
            self.selectable_text.push_str(gap);
        }
    }

    fn push_fragment(&mut self, mut fragment: SelectableFragment) {
        if fragment.text.is_empty() {
            return;
        }
        let start = self.selectable_text.len();
        self.selectable_text.push_str(&fragment.text);
        fragment.range = start..self.selectable_text.len();
        self.selectable_fragments.push(fragment);
    }

    fn clamp_selection_to_text(&mut self) {
        let len = self.selectable_text.len();
        if self.selection_anchor.is_some_and(|pos| pos > len) {
            self.selection_anchor = Some(len);
        }
        if self.selection_cursor.is_some_and(|pos| pos > len) {
            self.selection_cursor = Some(len);
        }
    }

    pub(super) fn text_pos_at(&self, pos: Point) -> Option<usize> {
        let mut best: Option<(f64, usize)> = None;
        for fragment in &self.selectable_fragments {
            if pos.y < fragment.y || pos.y > fragment.y + fragment.height {
                continue;
            }
            if let Some(clip) = fragment.clip {
                if pos.x < clip.x
                    || pos.x > clip.x + clip.width
                    || pos.y < clip.y
                    || pos.y > clip.y + clip.height
                {
                    continue;
                }
            }

            let x0 = fragment.text_x;
            let x1 = fragment.text_x + fragment.width;
            let (distance, offset) = if pos.x <= x0 {
                (x0 - pos.x, fragment.range.start)
            } else if pos.x >= x1 {
                (pos.x - x1, fragment.range.end)
            } else if matches!(fragment.kind, SelectableKind::Image { .. }) {
                let mid = x0 + fragment.width * 0.5;
                if pos.x < mid {
                    (0.0, fragment.range.start)
                } else {
                    (0.0, fragment.range.end)
                }
            } else {
                (
                    0.0,
                    fragment.range.start
                        + byte_at_x(
                            &self.active_font(),
                            &fragment.text,
                            fragment.font_size,
                            pos.x - x0,
                        ),
                )
            };

            if best.map_or(true, |(best_distance, _)| distance < best_distance) {
                best = Some((distance, offset));
            }
        }
        best.map(|(_, pos)| pos)
    }

    pub(super) fn selection_range(&self) -> Option<Range<usize>> {
        let anchor = self.selection_anchor?;
        let cursor = self.selection_cursor?;
        if anchor == cursor {
            None
        } else {
            Some(anchor.min(cursor)..anchor.max(cursor))
        }
    }

    pub(super) fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.selection_cursor = None;
    }

    pub(super) fn select_all_text(&mut self) {
        self.selection_anchor = Some(0);
        self.selection_cursor = Some(self.selectable_text.len());
    }

    pub(super) fn copy_selection(&self) {
        if let Some(range) = self.selection_range() {
            let (markdown, html) = self.copy_payloads(range);
            clipboard::set_rich_text(&markdown, &html);
        }
    }

    fn copy_payloads(&self, range: Range<usize>) -> (String, String) {
        let mut markdown = String::new();
        let mut cursor = range.start;

        for fragment in &self.selectable_fragments {
            if fragment.range.end <= range.start {
                continue;
            }
            if fragment.range.start >= range.end {
                break;
            }

            let gap_end = fragment.range.start.min(range.end);
            if cursor < gap_end {
                let gap = &self.selectable_text[cursor..gap_end];
                markdown.push_str(gap);
            }

            let start = range.start.max(fragment.range.start);
            let end = range.end.min(fragment.range.end);
            if start < end {
                let local_start = start - fragment.range.start;
                let local_end = end - fragment.range.start;
                let selected = &fragment.text[local_start..local_end];
                fragment.push_markdown(selected, start == fragment.range.start, &mut markdown);
            }
            cursor = end;
        }

        if cursor < range.end {
            let gap = &self.selectable_text[cursor..range.end];
            markdown.push_str(gap);
        }

        let html =
            super::rich_html::copy_html(&self.selectable_fragments, &self.selectable_text, range);
        (markdown, html)
    }

    pub(super) fn paint_selection(&self, ctx: &mut dyn DrawCtx) {
        let Some(range) = self.selection_range() else {
            return;
        };
        let color = if self.focused {
            ctx.visuals().selection_bg
        } else {
            ctx.visuals().selection_bg_unfocused
        };
        ctx.set_fill_color(color);
        ctx.set_font(Arc::clone(&self.active_font()));

        for fragment in &self.selectable_fragments {
            if fragment.range.end <= range.start || fragment.range.start >= range.end {
                continue;
            }

            let start = range.start.max(fragment.range.start) - fragment.range.start;
            let end = range.end.min(fragment.range.end) - fragment.range.start;
            let (x, w) = if matches!(fragment.kind, SelectableKind::Image { .. }) {
                (fragment.text_x, fragment.width)
            } else {
                ctx.set_font_size(fragment.font_size);
                let before = &fragment.text[..start];
                let selected = &fragment.text[start..end];
                (
                    fragment.text_x
                        + measure_advance(&self.active_font(), before, fragment.font_size),
                    measure_advance(&self.active_font(), selected, fragment.font_size),
                )
            };

            if w <= 0.0 {
                continue;
            }

            if let Some(clip) = fragment.clip {
                ctx.save();
                ctx.clip_rect(clip.x, clip.y, clip.width, clip.height);
                fill_highlight(ctx, x, fragment.y, w, fragment.height);
                ctx.restore();
            } else {
                fill_highlight(ctx, x, fragment.y, w, fragment.height);
            }
        }
    }

    pub(super) fn hit_image(&self, pos: Point) -> Option<(String, String, usize)> {
        for fragment in &self.selectable_fragments {
            let SelectableKind::Image {
                url,
                alt,
                cache_idx,
            } = &fragment.kind
            else {
                continue;
            };
            if pos.x >= fragment.text_x
                && pos.x <= fragment.text_x + fragment.width
                && pos.y >= fragment.y
                && pos.y <= fragment.y + fragment.height
            {
                return Some((url.clone(), alt.clone(), *cache_idx));
            }
        }
        None
    }
}

impl SelectableFragment {
    fn push_markdown(&self, selected: &str, starts_at_fragment: bool, out: &mut String) {
        match &self.kind {
            SelectableKind::Text {
                style: _,
                link,
                code,
            } => {
                if starts_at_fragment {
                    if let Some(prefix) = self.line_prefix {
                        out.push_str(prefix);
                    }
                }
                if let Some(url) = link {
                    push_wrapped_markdown(selected, out, |inner, out| {
                        out.push('[');
                        out.push_str(inner);
                        out.push_str("](");
                        out.push_str(url);
                        out.push(')');
                    });
                } else if *code {
                    push_wrapped_markdown(selected, out, |inner, out| {
                        out.push('`');
                        out.push_str(inner);
                        out.push('`');
                    });
                } else {
                    out.push_str(selected);
                }
            }
            SelectableKind::CodeBlock | SelectableKind::TableCell => out.push_str(selected),
            SelectableKind::Image { url, alt, .. } => {
                out.push_str("![");
                out.push_str(if alt.is_empty() { selected } else { alt });
                out.push_str("](");
                out.push_str(url);
                out.push(')');
            }
        }
    }
}

fn byte_at_x(font: &Arc<Font>, text: &str, font_size: f64, x: f64) -> usize {
    if x <= 0.0 {
        return 0;
    }
    let mut last = 0;
    for (idx, ch) in text.char_indices() {
        let next = idx + ch.len_utf8();
        let left = measure_advance(font, &text[..idx], font_size);
        let right = measure_advance(font, &text[..next], font_size);
        if x < (left + right) * 0.5 {
            return idx;
        }
        last = next;
    }
    last
}

fn fill_highlight(ctx: &mut dyn DrawCtx, x: f64, y: f64, w: f64, h: f64) {
    ctx.begin_path();
    ctx.rect(x, y + h * 0.12, w, h * 0.76);
    ctx.fill();
}

fn heading_prefix(style: LineStyle) -> Option<&'static str> {
    match style {
        LineStyle::H1 => Some("# "),
        LineStyle::H2 => Some("## "),
        LineStyle::H3 => Some("### "),
        LineStyle::H4 => Some("#### "),
        _ => None,
    }
}

pub(super) fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(super) fn escape_attr(text: &str) -> String {
    escape_html(text).replace('"', "&quot;")
}

fn push_wrapped_markdown(selected: &str, out: &mut String, wrap: impl FnOnce(&str, &mut String)) {
    let leading_len = selected.len() - selected.trim_start().len();
    let trailing_len = selected.len() - selected.trim_end().len();
    let content_start = leading_len;
    let content_end = selected.len().saturating_sub(trailing_len);
    out.push_str(&selected[..content_start]);
    if content_start < content_end {
        wrap(&selected[content_start..content_end], out);
    }
    out.push_str(&selected[content_end..]);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::geometry::Size;
    use crate::text::Font;

    use super::*;

    const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

    fn test_font() -> Arc<Font> {
        Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
    }

    #[test]
    fn html_escapes_text_and_gaps() {
        assert_eq!(escape_html("<a&b>"), "&lt;a&amp;b&gt;");
    }

    #[test]
    fn copy_payloads_include_markdown_and_rendered_html() {
        crate::widget::set_current_viewport(Size::new(480.0, 320.0));
        let mut view = MarkdownView::new(
            "# Title\n\nParagraph with [link](https://example.com) and `code`.\n\n```text\nlet x = 1;\n```\n\n| A | B |\n| --- | --- |\n| C | D |\n\n![Alt](image.png)",
            test_font(),
        );
        view.layout_markdown(Size::new(480.0, 2000.0));
        view.select_all_text();

        let (markdown, html) = view.copy_payloads(view.selection_range().expect("selection"));

        assert!(markdown.contains("# Title"));
        assert!(markdown.contains("[link](https://example.com)"));
        assert!(markdown.contains("`code`"));
        assert!(markdown.contains("let x = 1;"));
        assert!(markdown.contains("C\tD"));
        assert!(markdown.contains("![Alt](image.png)"));
        assert!(!html.contains("<h1>"));
        assert!(html.contains("font-size:"));
        assert!(html.contains("<table"));
        assert!(html.contains("<th"));
        assert!(!html.contains("<!--StartFragment-->"));
        assert!(html.contains("<a href=\"https://example.com\""));
        assert!(html.contains(">link</a>"));
        assert!(html.contains(">code</code>"));
        assert!(html.contains("<pre style="));
        assert!(html.contains("let x = 1;"));
        assert!(html.contains("<img src=\"image.png\" alt=\"Alt\""));
    }

    #[test]
    fn hit_testing_clamps_to_fragment_edges() {
        crate::widget::set_current_viewport(Size::new(320.0, 200.0));
        let mut view = MarkdownView::new("café", test_font());
        view.layout_markdown(Size::new(320.0, 1000.0));
        let fragment = view
            .selectable_fragments
            .iter()
            .find(|fragment| fragment.text.contains("café"))
            .expect("text fragment")
            .clone();

        let y = fragment.y + fragment.height * 0.5;
        assert_eq!(
            view.text_pos_at(Point::new(fragment.text_x - 20.0, y)),
            Some(fragment.range.start)
        );
        assert_eq!(
            view.text_pos_at(Point::new(fragment.text_x + fragment.width + 20.0, y)),
            Some(fragment.range.end)
        );
    }

    #[test]
    fn image_hit_test_targets_inline_images() {
        crate::widget::set_current_viewport(Size::new(320.0, 200.0));
        let mut view = MarkdownView::new("![Alt](image.png)", test_font());
        view.layout_markdown(Size::new(320.0, 1000.0));
        let fragment = view
            .selectable_fragments
            .iter()
            .find(|fragment| matches!(fragment.kind, SelectableKind::Image { .. }))
            .expect("image fragment")
            .clone();
        let hit = view.hit_image(Point::new(
            fragment.text_x + fragment.width * 0.5,
            fragment.y + fragment.height * 0.5,
        ));

        assert_eq!(hit, Some(("image.png".to_string(), "Alt".to_string(), 0)));
    }

    #[test]
    fn copy_payloads_render_ordered_and_bullet_lists_as_html_lists() {
        crate::widget::set_current_viewport(Size::new(480.0, 320.0));
        let mut view = MarkdownView::new(
            "1. First item\n2. Second item with [link](https://example.com)\n\n- Alpha\n- Beta",
            test_font(),
        );
        view.layout_markdown(Size::new(480.0, 2000.0));
        view.select_all_text();

        let (_, html) = view.copy_payloads(view.selection_range().expect("selection"));

        assert!(html.contains("<ol"));
        assert!(html.contains("<li"));
        assert!(html.contains(">First item"));
        assert!(html.contains(">Second item"));
        assert!(html.contains("<a href=\"https://example.com\""));
        assert!(html.contains("<ul"));
        assert!(html.contains(">Alpha"));
        assert!(html.contains(">Beta"));
        assert!(!html.contains(">1. First item"));
        assert!(!html.contains(">• Alpha"));
    }
}
