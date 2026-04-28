//! Painting for `MarkdownView`.
//!
//! All drawing in this module uses the already laid-out Y-up row positions
//! produced by `layout.rs`.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::text::measure_text_metrics;

use super::{
    image_loader, is_rect_visible_in_root, ImageState, LayoutItem, LineRun, LineStyle, MarkdownView,
};

impl MarkdownView {
    pub(super) fn paint_markdown(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let pad = self.padding;
        let w = self.bounds.width;
        let font = self.active_font();
        ctx.set_font(Arc::clone(&font));

        for item in &self.items {
            match item {
                LayoutItem::Line {
                    runs,
                    style,
                    indent,
                    quote,
                    y,
                    height,
                } => {
                    let fs = style.font_size(self.font_size);
                    ctx.set_font_size(fs);

                    let tx = pad + indent;
                    let ty = y + height * 0.5;
                    let metrics = measure_text_metrics(&font, "", fs);
                    let text_y = ty - (metrics.ascent - metrics.descent) * 0.5;
                    if *quote {
                        ctx.set_fill_color(v.separator);
                        ctx.begin_path();
                        ctx.rect(pad + indent - 12.0, *y, 3.0, *height);
                        ctx.fill();
                    }

                    match style {
                        LineStyle::Rule => {
                            ctx.set_fill_color(v.separator);
                            ctx.begin_path();
                            ctx.rect(pad, ty, w - pad * 2.0, 1.0);
                            ctx.fill();
                        }
                        LineStyle::Code => {
                            ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.15));
                            ctx.begin_path();
                            ctx.rounded_rect(pad, *y, w - pad * 2.0, *height, 3.0);
                            ctx.fill();
                            ctx.set_fill_color(v.accent);
                            for run in runs {
                                if let LineRun::Text { text, x, .. } = run {
                                    ctx.fill_text(text, tx + x + 4.0, text_y);
                                }
                            }
                        }
                        _ => {
                            ctx.set_fill_color(v.text_color);
                            for run in runs {
                                match run {
                                    LineRun::Text {
                                        text,
                                        link,
                                        code,
                                        x,
                                        width,
                                    } => {
                                        if *code {
                                            ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.5, 0.14));
                                            ctx.begin_path();
                                            ctx.rounded_rect(
                                                tx + x,
                                                *y + height * 0.16,
                                                *width,
                                                height * 0.68,
                                                3.0,
                                            );
                                            ctx.fill();
                                        }
                                        ctx.set_fill_color(if link.is_some() {
                                            v.accent
                                        } else if *code {
                                            v.text_color
                                        } else {
                                            v.text_color
                                        });
                                        let text_x = if *code {
                                            tx + x + self.font_size * 0.35
                                        } else {
                                            tx + x
                                        };
                                        ctx.fill_text(text, text_x, text_y);
                                        if link.is_some() {
                                            ctx.begin_path();
                                            ctx.rect(tx + x, text_y - 2.0, *width, 1.0);
                                            ctx.fill();
                                        }
                                    }
                                    LineRun::Image {
                                        alt,
                                        link: _,
                                        cache_idx,
                                        x,
                                        y_offset,
                                        width,
                                        height,
                                    } => {
                                        let rx = tx + x;
                                        let ry = y + y_offset;
                                        if let Some(entry) = self.image_cache.get(*cache_idx) {
                                            if is_rect_visible_in_root(ctx, rx, ry, *width, *height)
                                            {
                                                let should_load = if let Ok(mut state) =
                                                    entry.state.lock()
                                                {
                                                    if matches!(*state, ImageState::RemotePending) {
                                                        *state = ImageState::Loading;
                                                        true
                                                    } else {
                                                        false
                                                    }
                                                } else {
                                                    false
                                                };
                                                if should_load {
                                                    image_loader::load_remote_image(
                                                        entry.url.clone(),
                                                        std::sync::Arc::clone(&entry.state),
                                                    );
                                                }
                                            }
                                            let ready = entry.state.lock().ok().and_then(|state| {
                                                match &*state {
                                                    ImageState::Ready { image, .. } => {
                                                        Some(image.clone())
                                                    }
                                                    _ => None,
                                                }
                                            });
                                            if let Some(image) = ready {
                                                ctx.draw_image_rgba(
                                                    image.data.as_slice(),
                                                    image.width,
                                                    image.height,
                                                    rx,
                                                    ry,
                                                    *width,
                                                    *height,
                                                );
                                            } else {
                                                ctx.set_fill_color(Color::rgba(
                                                    0.5, 0.5, 0.5, 0.15,
                                                ));
                                                ctx.begin_path();
                                                ctx.rounded_rect(rx, ry, *width, *height, 3.0);
                                                ctx.fill();
                                                ctx.set_fill_color(v.text_dim);
                                                ctx.set_font_size(self.font_size * 0.85);
                                                let label = if alt.is_empty() {
                                                    "image".to_string()
                                                } else {
                                                    alt.clone()
                                                };
                                                ctx.fill_text(&label, rx + 8.0, ry + height * 0.5);
                                                ctx.set_font_size(fs);
                                                ctx.set_fill_color(v.text_color);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if matches!(style, LineStyle::H1 | LineStyle::H2) && !runs.is_empty() {
                        ctx.set_fill_color(v.separator);
                        ctx.begin_path();
                        ctx.rect(pad, *y, w - pad * 2.0, 1.0);
                        ctx.fill();
                    }
                }
                LayoutItem::Table {
                    rows,
                    y,
                    row_h,
                    col_widths,
                    ..
                } => {
                    let table_w: f64 = col_widths.iter().sum();
                    let mut cy = y + row_h * rows.len() as f64;
                    ctx.set_font_size(self.font_size);
                    for (ri, row) in rows.iter().enumerate() {
                        cy -= row_h;
                        if ri == 0 {
                            ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.5, 0.10));
                            ctx.begin_path();
                            ctx.rect(pad, cy, table_w, *row_h);
                            ctx.fill();
                        }
                        let mut cx = pad;
                        for (ci, width) in col_widths.iter().enumerate() {
                            ctx.set_fill_color(v.separator);
                            ctx.begin_path();
                            ctx.rect(cx, cy, 1.0, *row_h);
                            ctx.rect(cx, cy, *width, 1.0);
                            ctx.fill();
                            if let Some(text) = row.get(ci) {
                                ctx.set_fill_color(v.text_color);
                                ctx.fill_text(text, cx + 8.0, cy + row_h * 0.36);
                            }
                            cx += width;
                        }
                        ctx.set_fill_color(v.separator);
                        ctx.begin_path();
                        ctx.rect(pad + table_w, cy, 1.0, *row_h);
                        ctx.fill();
                    }
                    ctx.set_fill_color(v.separator);
                    ctx.begin_path();
                    ctx.rect(pad, y + row_h * rows.len() as f64, table_w, 1.0);
                    ctx.fill();
                }
                LayoutItem::CodeBlock {
                    lines,
                    y,
                    height,
                    line_h,
                    width,
                } => {
                    let code_pad_x = self.font_size;
                    let code_pad_y = self.font_size * 0.75;
                    let fs = LineStyle::Code.font_size(self.font_size);
                    ctx.set_font_size(fs);
                    ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.5, 0.12));
                    ctx.begin_path();
                    ctx.rounded_rect(pad, *y, *width, *height, 4.0);
                    ctx.fill();
                    ctx.set_fill_color(v.text_color);

                    let metrics = measure_text_metrics(&font, "", fs);
                    let mut line_top = y + height - code_pad_y - line_h;
                    for line in lines {
                        let ty = line_top + line_h * 0.5;
                        let text_y = ty - (metrics.ascent - metrics.descent) * 0.5;
                        ctx.fill_text(line, pad + code_pad_x, text_y);
                        line_top -= line_h;
                    }
                }
            }
        }
    }
}
