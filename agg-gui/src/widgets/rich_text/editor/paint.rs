//! Painting for [`RichTextEdit`], split out of `editor.rs`.
//!
//! Walks the cached [`DocLayout`] top-down and flips into the framework's Y-up
//! space (folding in padding + scroll via
//! [`doc_top_yup`](RichTextEdit::doc_top_yup)), mirroring the read-only
//! [`RichTextView`](super::super::view::RichTextView) painter but adding the
//! selection band, a focus border, and a blinking caret drawn in the overlay
//! pass (clipped to the inner band, `TextArea`-style).

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;

use super::super::layout::LineLayout;
use super::RichTextEdit;

impl RichTextEdit {
    /// Paint background, selection, text runs, decorations, border and bar.
    pub(super) fn paint_content(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let default_color = v.text_color;

        // Background.
        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
        ctx.fill();

        // Clip to the padded inner rect so scrolled content can't leak.
        ctx.clip_rect(
            self.padding,
            self.padding,
            (w - self.padding * 2.0).max(0.0),
            (h - self.padding * 2.0).max(0.0),
        );

        let Some(layout) = self.layout.clone() else {
            ctx.reset_clip();
            return;
        };
        let sel = self.core.borrow().selection();
        let (a, b) = sel.ordered();
        let sel_active = !sel.is_empty();
        let sel_color = if self.focused {
            v.selection_bg
        } else {
            v.selection_bg_unfocused
        };

        let mut y_top = 0.0f64;
        for (bi, block) in layout.blocks.iter().enumerate() {
            // List marker on the first line's baseline.
            if let (Some(marker), Some(font), Some(first)) =
                (&block.marker, &block.marker_font, block.lines.first())
            {
                let baseline_up = crate::font_settings::snap_baseline_y(
                    self.doc_top_yup() - (y_top + first.baseline_from_top),
                );
                let color = first
                    .fragments
                    .first()
                    .and_then(|f| f.style.text_color)
                    .unwrap_or(default_color);
                ctx.set_font(Arc::clone(font));
                ctx.set_font_size(block.marker_font_size);
                ctx.set_fill_color(color);
                ctx.fill_text(marker, self.padding + block.marker_x, baseline_up);
            }

            for line in &block.lines {
                let line_top = y_top;
                let baseline_up = crate::font_settings::snap_baseline_y(
                    self.doc_top_yup() - (line_top + line.baseline_from_top),
                );
                let line_bottom_up = self.doc_top_yup() - (line_top + line.height);
                let text_x0 = self.padding + block.text_left + line.align_dx;

                // Selection band behind the line.
                if sel_active && bi >= a.block && bi <= b.block {
                    let block_lo = if bi == a.block { a.byte } else { 0 };
                    let block_hi = if bi == b.block {
                        b.byte
                    } else {
                        line.end_byte.max(line.start_byte)
                    };
                    let lo = line.start_byte.max(block_lo);
                    let hi = line.end_byte.min(block_hi);
                    if hi >= lo {
                        let x0 = text_x0 + self.x_in_line(line, lo);
                        let x1 = text_x0 + self.x_in_line(line, hi);
                        let width = (x1 - x0).max(if bi < b.block { 4.0 } else { 0.0 });
                        if width > 0.0 {
                            ctx.set_fill_color(sel_color);
                            ctx.begin_path();
                            ctx.rect(x0, line_bottom_up, width, line.height);
                            ctx.fill();
                        }
                    }
                }

                self.paint_line(ctx, line, text_x0, baseline_up, default_color);
                y_top += line.height;
            }
        }

        ctx.reset_clip();

        // Focus border.
        let border = if self.focused {
            v.accent
        } else if self.hovered {
            v.widget_stroke_active
        } else {
            v.widget_stroke
        };
        let line_width = if self.focused { 2.0 } else { 1.0 };
        ctx.set_stroke_color(border);
        ctx.set_line_width(line_width);
        ctx.begin_path();
        let inset = line_width * 0.5;
        ctx.rounded_rect(
            inset,
            inset,
            (w - line_width).max(0.0),
            (h - line_width).max(0.0),
            4.0,
        );
        ctx.stroke();
        // Scrollbar paints in `paint_overlay` (after the cache blit) so its
        // animation doesn't force the styled-text bitmap to re-raster.
    }

    /// Paint the fragments of one line (text, highlight, underline/strike).
    fn paint_line(
        &self,
        ctx: &mut dyn DrawCtx,
        line: &LineLayout,
        text_x0: f64,
        baseline_up: f64,
        default_color: crate::color::Color,
    ) {
        let line_bottom_up = baseline_up + line.baseline_from_top - line.height;
        for frag in &line.fragments {
            let fx = text_x0 + frag.x;
            if let Some(hl) = frag.style.highlight {
                ctx.set_fill_color(hl);
                ctx.begin_path();
                ctx.rect(fx, line_bottom_up, frag.width, line.height);
                ctx.fill();
            }
            let color = frag.style.text_color.unwrap_or(default_color);
            ctx.set_font(Arc::clone(&frag.font));
            ctx.set_font_size(frag.font_size);
            ctx.set_fill_color(color);
            ctx.fill_text(&frag.text, fx, baseline_up);

            if frag.style.underline || frag.style.strikethrough {
                ctx.set_stroke_color(color);
                ctx.set_line_width(1.0_f64.max(frag.font_size * 0.06));
            }
            if frag.style.underline {
                let uy = baseline_up - (frag.font_size * 0.12).max(1.5);
                stroke_hline(ctx, fx, fx + frag.width, uy);
            }
            if frag.style.strikethrough {
                let sy = baseline_up + frag.font_size * 0.28;
                stroke_hline(ctx, fx, fx + frag.width, sy);
            }
        }
    }

    /// Paint the blinking caret in the overlay pass (clipped to the inner band).
    pub(super) fn paint_caret(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.focused {
            return;
        }
        if let Some(t) = self.focus_time {
            let phase = (t.elapsed().as_millis() / 500) as u64;
            self.blink_last_phase.set(phase);
            if phase % 2 == 1 {
                return;
            }
        }
        let caret = self.core.borrow().caret();
        let Some(geom) = self.caret_geometry(caret) else {
            return;
        };
        // Clamp the caret's vertical span to the padded inner band so it never
        // streaks over the border when its line has scrolled out of view.
        let inner_lo = self.padding;
        let inner_hi = (self.bounds.height - self.padding).max(inner_lo);
        let y0 = geom.y_bottom.max(inner_lo);
        let y1 = (geom.y_bottom + geom.height).min(inner_hi);
        if y1 <= y0 {
            return;
        }
        let v = ctx.visuals();
        ctx.set_stroke_color(v.text_color);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.move_to(geom.x, y0);
        ctx.line_to(geom.x, y1);
        ctx.stroke();
    }
}

/// Stroke a 1px-ish horizontal line at Y-up `y` from `x0` to `x1`.
fn stroke_hline(ctx: &mut dyn DrawCtx, x0: f64, x1: f64, y: f64) {
    ctx.begin_path();
    ctx.move_to(x0, y);
    ctx.line_to(x1, y);
    ctx.stroke();
}
