#![allow(unused_imports)]
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::framebuffer::unpremultiply_rgba_inplace;
use agg_gui::widget::paint_subtree;
use agg_gui::{
    render_svg_at_size, render_svg_to_framebuffer_at_size_with_resources,
    render_svg_to_lcd_buffer_at_size_with_resources, set_cursor_icon, Color, Container, CursorIcon,
    DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, Hyperlink, Label, MouseButton, Point,
    Rect, Resize, ScrollBarVisibility, ScrollView, Separator, Size, SizedBox, TextArea, TextField,
    Visuals, Widget,
};

mod controls;
mod drawing;
mod samples;
#[cfg(test)]
mod svg_tests;

use drawing::{
    decode_png_rgba, draw_hardware_column, draw_lcd_column, draw_panel, draw_raster_column,
    draw_small_text, native_rect,
};
use samples::{SvgSample, SVG_SAMPLES};

// SVG Test
// ---------------------------------------------------------------------------

/// Build the SVG Test — live progress viewer for the library SVG renderer.
pub fn svg_test(font: Arc<Font>) -> Box<dyn Widget> {
    let samples = Arc::new(
        SVG_SAMPLES
            .iter()
            .map(SvgSampleRender::new)
            .collect::<Vec<_>>(),
    );
    let zoom = Rc::new(Cell::new(SVG_DEFAULT_ZOOM));
    let v_offset = Rc::new(Cell::new(0.0));
    let v_max = Rc::new(Cell::new(0.0));
    let h_offset = Rc::new(Cell::new(0.0));
    let h_max = Rc::new(Cell::new(0.0));

    let mut root = FlexColumn::new()
        .with_gap(0.0)
        .with_padding(0.0)
        .with_panel_bg();
    root.push(
        Box::new(SvgProgressHeader::new(
            Arc::clone(&font),
            Arc::clone(&samples),
            Rc::clone(&zoom),
            Rc::clone(&v_offset),
            Rc::clone(&v_max),
            Rc::clone(&h_offset),
            Rc::clone(&h_max),
        )),
        0.0,
    );
    root.push(
        Box::new(
            ScrollView::new(Box::new(SvgProgressBody::new(
                Arc::clone(&font),
                Arc::clone(&samples),
                Rc::clone(&zoom),
                Rc::clone(&v_offset),
                Rc::clone(&v_max),
                Rc::clone(&h_offset),
                Rc::clone(&h_max),
            )))
            .horizontal(true)
            .with_offset_cell(Rc::clone(&v_offset))
            .with_max_scroll_cell(Rc::clone(&v_max))
            .with_h_offset_cell(Rc::clone(&h_offset))
            .with_h_max_scroll_cell(Rc::clone(&h_max))
            .with_bar_visibility(ScrollBarVisibility::AlwaysVisible),
        ),
        1.0,
    );

    Box::new(root)
}

const SVG_HEADER_H: f64 = 124.0;
const SVG_TITLE_H: f64 = 92.0;
const SVG_COLUMN_HEADER_H: f64 = 32.0;
const SVG_PAD: f64 = 8.0;
const SVG_GAP: f64 = 8.0;
const SVG_DEFAULT_ZOOM: f64 = 0.5;
const SVG_MIN_ZOOM: f64 = 0.1;
const SVG_MAX_ZOOM: f64 = 8.0;

struct SvgProgressHeader {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    samples: Arc<Vec<SvgSampleRender>>,
    zoom: Rc<Cell<f64>>,
}

struct SvgProgressBody {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    samples: Arc<Vec<SvgSampleRender>>,
    zoom: Rc<Cell<f64>>,
    v_offset: Rc<Cell<f64>>,
    v_max: Rc<Cell<f64>>,
    h_offset: Rc<Cell<f64>>,
    h_max: Rc<Cell<f64>>,
    diff_row_held: Option<usize>,
}

struct SvgZoomButton {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    label: &'static str,
    target_zoom: Option<f64>,
    font: Arc<Font>,
    samples: Arc<Vec<SvgSampleRender>>,
    zoom: Rc<Cell<f64>>,
    v_offset: Rc<Cell<f64>>,
    v_max: Rc<Cell<f64>>,
    h_offset: Rc<Cell<f64>>,
    h_max: Rc<Cell<f64>>,
    pressed: bool,
    hovered: bool,
}

struct SvgSampleRender {
    name: &'static str,
    svg: &'static [u8],
    width: u32,
    height: u32,
    reference: Result<Arc<Vec<u8>>, String>,
    rgba: Result<Arc<Vec<u8>>, String>,
    rgba_diff: Result<Arc<Vec<u8>>, String>,
    rgba_exact: bool,
    lcd: Result<SvgLcdPreview, String>,
}

struct SvgLcdPreview {
    color: Arc<Vec<u8>>,
    alpha: Arc<Vec<u8>>,
}

impl SvgProgressHeader {
    fn new(
        font: Arc<Font>,
        samples: Arc<Vec<SvgSampleRender>>,
        zoom: Rc<Cell<f64>>,
        v_offset: Rc<Cell<f64>>,
        v_max: Rc<Cell<f64>>,
        h_offset: Rc<Cell<f64>>,
        h_max: Rc<Cell<f64>>,
    ) -> Self {
        let mut children: Vec<Box<dyn Widget>> = Vec::new();
        for (label, target_zoom) in [("50%", 0.5), ("100%", 1.0)] {
            children.push(Box::new(SvgZoomButton::new(
                label,
                Some(target_zoom),
                Arc::clone(&font),
                Arc::clone(&samples),
                Rc::clone(&zoom),
                Rc::clone(&v_offset),
                Rc::clone(&v_max),
                Rc::clone(&h_offset),
                Rc::clone(&h_max),
            )));
        }
        children.push(Box::new(SvgZoomButton::new(
            "Custom",
            None,
            Arc::clone(&font),
            Arc::clone(&samples),
            Rc::clone(&zoom),
            Rc::clone(&v_offset),
            Rc::clone(&v_max),
            Rc::clone(&h_offset),
            Rc::clone(&h_max),
        )));
        Self {
            bounds: Rect::default(),
            children,
            font,
            samples,
            zoom,
        }
    }
}

impl SvgProgressBody {
    fn new(
        font: Arc<Font>,
        samples: Arc<Vec<SvgSampleRender>>,
        zoom: Rc<Cell<f64>>,
        v_offset: Rc<Cell<f64>>,
        v_max: Rc<Cell<f64>>,
        h_offset: Rc<Cell<f64>>,
        h_max: Rc<Cell<f64>>,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            samples,
            zoom,
            v_offset,
            v_max,
            h_offset,
            h_max,
            diff_row_held: None,
        }
    }
}

impl SvgSampleRender {
    fn new(sample: &SvgSample) -> Self {
        let reference = decode_png_rgba(sample.reference_png);
        let (width, height) = reference
            .as_ref()
            .map(|(_, w, h)| (*w, *h))
            .unwrap_or((1, 1));
        let resources_dir = svg_sample_resource_dir(sample.name);

        let rgba = render_svg_to_framebuffer_at_size_with_resources(
            sample.svg,
            width,
            height,
            &resources_dir,
        )
        .map(|fb| {
            let mut pixels = fb.pixels_flipped();
            unpremultiply_rgba_inplace(&mut pixels);
            Arc::new(pixels)
        })
        .map_err(|e| e.to_string());

        let lcd = render_svg_to_lcd_buffer_at_size_with_resources(
            sample.svg,
            width,
            height,
            &resources_dir,
        )
        .map(|buffer| SvgLcdPreview {
            color: Arc::new(buffer.color_plane_flipped()),
            alpha: Arc::new(buffer.alpha_plane_flipped()),
        })
        .map_err(|e| e.to_string());
        let rgba_diff = match (&reference, &rgba) {
            (Ok((reference, _, _)), Ok(rgba)) => Ok(Arc::new(diff_rgba_pixels(reference, rgba))),
            (Err(err), _) => Err(format!("reference: {err}")),
            (_, Err(err)) => Err(format!("rgba: {err}")),
        };
        let rgba_exact = match (&reference, &rgba) {
            (Ok((reference, _, _)), Ok(rgba)) => reference.as_slice() == rgba.as_slice(),
            _ => false,
        };

        Self {
            name: sample.name,
            svg: sample.svg,
            width,
            height,
            reference: reference.map(|(pixels, _, _)| Arc::new(pixels)),
            rgba,
            rgba_diff,
            rgba_exact,
            lcd,
        }
    }
}

fn diff_rgba_pixels(reference: &[u8], rendered: &[u8]) -> Vec<u8> {
    reference
        .chunks_exact(4)
        .zip(rendered.chunks_exact(4))
        .flat_map(|(reference, rendered)| {
            let dr = reference[0].abs_diff(rendered[0]);
            let dg = reference[1].abs_diff(rendered[1]);
            let db = reference[2].abs_diff(rendered[2]);
            let da = reference[3].abs_diff(rendered[3]);
            [dr.max(da), dg.max(da), db.max(da), 255]
        })
        .collect()
}

fn svg_sample_resource_dir(name: &str) -> PathBuf {
    let suite_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("demo-ui should live under workspace root")
        .join("tests/resvg-test-suite/tests");
    match Path::new(name).parent() {
        Some(parent) => suite_root.join(parent),
        None => suite_root,
    }
}

impl Widget for SvgProgressHeader {
    fn type_name(&self) -> &'static str {
        "SvgProgressHeader"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, SVG_HEADER_H);
        let mut x = SVG_PAD + 2.0;
        for child in &mut self.children {
            let size = child.layout(Size::new(78.0, 22.0));
            child.set_bounds(Rect::new(x, SVG_HEADER_H - 80.0, size.width, 22.0));
            x += size.width + 6.0;
        }
        Size::new(available.width, SVG_HEADER_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_font(Arc::clone(&self.font));
        let w = self.bounds.width;
        let h = self.bounds.height.max(self.min_content_height());
        let zoom = self.zoom.get();
        let col_w = column_width(&self.samples, w, zoom);
        let titles = [
            "reference.png / control",
            "agg-rgba-bitmap render",
            "agg-lcd-bitmap render",
            "hardware render",
        ];

        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        let title_y = h - 22.0;
        draw_small_text(
            ctx,
            "SVG renderer progress viewer",
            SVG_PAD + 2.0,
            title_y,
            13.0,
            v.text_color,
        );
        draw_small_text(
            ctx,
            "Headers are fixed; reference.png is from resvg-test-suite and every output is rendered/displayed at that native pixel size.",
            SVG_PAD + 2.0,
            title_y - 18.0,
            10.5,
            v.text_dim,
        );
        draw_small_text(
            ctx,
            &format!("Zoom: {:.0}%", zoom * 100.0),
            SVG_PAD + 2.0,
            h - 54.0,
            10.5,
            v.text_dim,
        );
        draw_small_text(
            ctx,
            "Ctrl+wheel zooms at cursor",
            SVG_PAD + 184.0,
            h - 74.0,
            10.5,
            v.text_dim,
        );

        let header_y = h - SVG_TITLE_H - 22.0;
        ctx.set_fill_color(v.window_title_fill);
        ctx.begin_path();
        ctx.rect(
            0.0,
            h - SVG_TITLE_H - SVG_COLUMN_HEADER_H,
            w,
            SVG_COLUMN_HEADER_H,
        );
        ctx.fill();
        for (i, title) in titles.iter().enumerate() {
            let x = SVG_PAD + i as f64 * (col_w + SVG_GAP);
            draw_small_text(ctx, title, x + 6.0, header_y, 10.5, v.text_color);
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

impl Widget for SvgProgressBody {
    fn type_name(&self) -> &'static str {
        "SvgProgressBody"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, _: Size) -> Size {
        let zoom = self.zoom.get();
        let size = Size::new(
            self.min_content_width_at(zoom),
            self.min_content_height_at(zoom),
        );
        self.bounds = Rect::new(0.0, 0.0, size.width, size.height);
        size
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_font(Arc::clone(&self.font));
        let zoom = self.zoom.get();
        let w = self.bounds.width.max(self.min_content_width_at(zoom));
        let h = self.bounds.height.max(self.min_content_height_at(zoom));
        let row_h = self.row_height_at(zoom);
        let col_w = column_width(&self.samples, w, zoom);

        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        for (row, sample) in self.samples.iter().enumerate() {
            let row_top = h - SVG_PAD - row as f64 * row_h;
            let y = row_top - row_h + 6.0;
            let title_x = SVG_PAD + 6.0;
            let title_baseline_y = row_top - 17.0;
            draw_small_text(
                ctx,
                sample.name,
                title_x,
                title_baseline_y,
                10.0,
                v.text_dim,
            );
            let (title_w, title_center_y) = title_metrics(ctx, sample.name, title_baseline_y);
            draw_status_icon(ctx, sample, title_x + title_w + 4.0, title_center_y - 4.0);

            for col in 0..4 {
                let x = SVG_PAD + col as f64 * (col_w + SVG_GAP);
                draw_panel(ctx, x, y, col_w, row_h - 26.0, &v);
                match col {
                    0 => draw_raster_column(
                        ctx,
                        &sample.reference,
                        sample.width,
                        sample.height,
                        zoom,
                        x,
                        y,
                        col_w,
                        row_h - 26.0,
                        &v,
                    ),
                    1 => {
                        let pixels = if self.diff_row_held == Some(row) {
                            &sample.rgba_diff
                        } else {
                            &sample.rgba
                        };
                        draw_raster_column(
                            ctx,
                            pixels,
                            sample.width,
                            sample.height,
                            zoom,
                            x,
                            y,
                            col_w,
                            row_h - 26.0,
                            &v,
                        );
                    }
                    2 => draw_lcd_column(
                        ctx,
                        &sample.lcd,
                        sample.width,
                        sample.height,
                        zoom,
                        x,
                        y,
                        col_w,
                        row_h - 26.0,
                        &v,
                    ),
                    3 => draw_hardware_column(ctx, sample, zoom, x, y, col_w, row_h - 26.0, &v),
                    _ => {}
                }
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseWheel {
                pos,
                delta_y,
                modifiers,
                ..
            } if modifiers.ctrl => {
                let old_zoom = self.zoom.get();
                let factor = (-delta_y * 0.1).exp();
                let new_zoom = (old_zoom * factor).clamp(SVG_MIN_ZOOM, SVG_MAX_ZOOM);
                zoom_svg_around_content_point(
                    &self.samples,
                    &self.zoom,
                    &self.v_offset,
                    &self.v_max,
                    &self.h_offset,
                    &self.h_max,
                    pos.x,
                    svg_content_height(&self.samples, old_zoom) - pos.y,
                    new_zoom,
                );
                EventResult::Consumed
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                self.diff_row_held = self.rgba_row_at(*pos);
                if self.diff_row_held.is_some() {
                    agg_gui::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                let was_holding = self.diff_row_held.take().is_some();
                if was_holding {
                    agg_gui::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }
}

impl SvgProgressHeader {
    fn min_content_height(&self) -> f64 {
        SVG_HEADER_H
    }
}

impl SvgProgressBody {
    fn row_height_at(&self, zoom: f64) -> f64 {
        self.samples
            .iter()
            .map(|sample| sample.height as f64 * zoom)
            .fold(90.0, f64::max)
            + 26.0
    }

    fn min_content_width_at(&self, zoom: f64) -> f64 {
        svg_content_width(&self.samples, zoom)
    }

    fn min_content_height_at(&self, zoom: f64) -> f64 {
        SVG_PAD * 2.0 + self.row_height_at(zoom) * self.samples.len().max(1) as f64
    }

    fn rgba_row_at(&self, pos: Point) -> Option<usize> {
        let zoom = self.zoom.get();
        let h = self.bounds.height.max(self.min_content_height_at(zoom));
        let row_h = self.row_height_at(zoom);
        let col_w = column_width(&self.samples, self.bounds.width, zoom);
        let col_x = SVG_PAD + col_w + SVG_GAP;
        for (row, sample) in self.samples.iter().enumerate() {
            let row_top = h - SVG_PAD - row as f64 * row_h;
            let panel_y = row_top - row_h + 6.0;
            let panel_h = row_h - 26.0;
            let (dx, dy, dw, dh) = native_rect(
                sample.width as f64 * zoom,
                sample.height as f64 * zoom,
                col_x,
                panel_y,
                col_w,
                panel_h,
            );
            if pos.x >= dx && pos.x <= dx + dw && pos.y >= dy && pos.y <= dy + dh {
                return Some(row);
            }
        }
        None
    }
}

fn title_metrics(ctx: &mut dyn DrawCtx, title: &str, baseline_y: f64) -> (f64, f64) {
    ctx.set_font_size(10.0);
    ctx.measure_text(title)
        .map(|metrics| {
            (
                metrics.width,
                baseline_y + (metrics.ascent - metrics.descent) * 0.5,
            )
        })
        .unwrap_or((title.len() as f64 * 5.5, baseline_y + 3.5))
}

fn draw_status_icon(ctx: &mut dyn DrawCtx, sample: &SvgSampleRender, x: f64, y: f64) {
    let (fill, stroke) = if sample.rgba_exact {
        (Color::rgb(0.2, 0.75, 0.25), Color::rgb(0.1, 0.45, 0.15))
    } else {
        (Color::rgb(0.85, 0.22, 0.22), Color::rgb(0.55, 0.08, 0.08))
    };
    ctx.set_fill_color(fill);
    ctx.set_stroke_color(stroke);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.circle(x + 4.0, y + 4.0, 4.0);
    ctx.fill_and_stroke();

    ctx.set_stroke_color(Color::white());
    ctx.set_line_width(1.2);
    ctx.begin_path();
    if sample.rgba_exact {
        ctx.move_to(x + 2.0, y + 4.0);
        ctx.line_to(x + 3.5, y + 2.5);
        ctx.line_to(x + 6.4, y + 5.9);
    } else {
        ctx.move_to(x + 2.4, y + 2.4);
        ctx.line_to(x + 5.6, y + 5.6);
        ctx.move_to(x + 5.6, y + 2.4);
        ctx.line_to(x + 2.4, y + 5.6);
    }
    ctx.stroke();
}

fn svg_content_width(samples: &[SvgSampleRender], zoom: f64) -> f64 {
    let max_sample_w = samples
        .iter()
        .map(|sample| sample.width as f64 * zoom)
        .fold(120.0, f64::max);
    SVG_PAD * 2.0 + (max_sample_w + 16.0) * 4.0 + SVG_GAP * 3.0
}

fn svg_content_height(samples: &[SvgSampleRender], zoom: f64) -> f64 {
    SVG_PAD * 2.0 + svg_row_height(samples, zoom) * samples.len().max(1) as f64
}

fn svg_row_height(samples: &[SvgSampleRender], zoom: f64) -> f64 {
    samples
        .iter()
        .map(|sample| sample.height as f64 * zoom)
        .fold(90.0, f64::max)
        + 26.0
}

fn column_width(samples: &[SvgSampleRender], available_width: f64, zoom: f64) -> f64 {
    let max_sample_w = samples
        .iter()
        .map(|sample| sample.width as f64 * zoom)
        .fold(120.0, f64::max);
    ((available_width - SVG_PAD * 2.0 - SVG_GAP * 3.0) / 4.0).max(max_sample_w + 16.0)
}

fn is_zoom_level(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() < 0.001
}

fn zoom_svg_around_viewport_center(
    samples: &[SvgSampleRender],
    zoom: &Rc<Cell<f64>>,
    v_offset: &Rc<Cell<f64>>,
    v_max: &Rc<Cell<f64>>,
    h_offset: &Rc<Cell<f64>>,
    h_max: &Rc<Cell<f64>>,
    new_zoom: f64,
) {
    let old_zoom = zoom.get();
    let old_w = svg_content_width(samples, old_zoom);
    let old_h = svg_content_height(samples, old_zoom);
    let viewport_w = (old_w - h_max.get()).max(1.0);
    let viewport_h = (old_h - v_max.get()).max(1.0);
    zoom_svg_around_content_point(
        samples,
        zoom,
        v_offset,
        v_max,
        h_offset,
        h_max,
        h_offset.get() + viewport_w * 0.5,
        v_offset.get() + viewport_h * 0.5,
        new_zoom,
    );
}

fn zoom_svg_around_content_point(
    samples: &[SvgSampleRender],
    zoom: &Rc<Cell<f64>>,
    v_offset: &Rc<Cell<f64>>,
    v_max: &Rc<Cell<f64>>,
    h_offset: &Rc<Cell<f64>>,
    h_max: &Rc<Cell<f64>>,
    anchor_x: f64,
    anchor_top_y: f64,
    new_zoom: f64,
) {
    let old_zoom = zoom.get();
    if (new_zoom - old_zoom).abs() < 0.001 {
        return;
    }

    let old_w = svg_content_width(samples, old_zoom);
    let old_h = svg_content_height(samples, old_zoom);
    let new_w = svg_content_width(samples, new_zoom);
    let new_h = svg_content_height(samples, new_zoom);
    let viewport_w = (old_w - h_max.get()).max(1.0);
    let viewport_h = (old_h - v_max.get()).max(1.0);
    let screen_x = anchor_x - h_offset.get();
    let screen_top_y = anchor_top_y - v_offset.get();
    let new_h_max = (new_w - viewport_w).max(0.0);
    let new_v_max = (new_h - viewport_h).max(0.0);
    let scaled_anchor_x = anchor_x * (new_w / old_w.max(1.0));
    let scaled_anchor_top_y = anchor_top_y * (new_h / old_h.max(1.0));

    zoom.set(new_zoom);
    h_max.set(new_h_max);
    v_max.set(new_v_max);
    h_offset.set((scaled_anchor_x - screen_x).clamp(0.0, new_h_max));
    v_offset.set((scaled_anchor_top_y - screen_top_y).clamp(0.0, new_v_max));
    agg_gui::animation::request_draw();
}
