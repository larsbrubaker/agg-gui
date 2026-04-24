//! Demo window content builders — dispatcher module.
//!
//! This file declares the submodules that contain the actual implementations
//! and re-exports every public builder function so that callers can write
//! `windows::widget_gallery(font)` etc. without knowing the submodule layout.
//!
//! Retained here (not delegated):
//! - `ComingSoon` struct + `coming_soon()` — used ubiquitously by `lib.rs`.
//! - `about()`, `load_png()`, `cube_content()` — tightly coupled to the demo
//!   shell and unlikely to grow beyond their current size.

mod gallery;
mod basic;
mod code_example;
mod animation;
mod font_book;
mod frame_demo;
mod lion;
mod misc;
mod interaction;
mod scrolling;
mod system;
mod text_demos;
mod tests;
mod truetype_lcd;

// Re-export every public demo builder so callers use `windows::foo(font)`.
pub use gallery::widget_gallery;
pub use basic::{sliders, text_edit, tooltips, code_editor};
pub use code_example::code_example;
pub use animation::{bezier_curve, dancing_strings, painting};
pub use font_book::font_book;
pub use frame_demo::frame_demo;
pub use lion::lion_demo;
pub use misc::{extra_viewport, highlighting, interactive_container, misc_demos};
pub use interaction::{drag_and_drop, panels_demo, popups_demo,
                      scene_demo, screenshot_demo};
pub use scrolling::scrolling_demo;
pub use system::{system_view, load_font_by_name, font_option_index,
                 font_option_names, load_all_fonts, apply_font_by_index,
                 default_font_index,
                 cells as system_cells, init_cells as init_system_cells, SystemCells};
pub use truetype_lcd::truetype_lcd_view;
pub use text_demos::{strip_demo, table_demo, text_layout, undo_redo,
                     window_options, modals_demo, multi_touch};
pub use tests::{clipboard_test, cursor_test, grid_test, id_test,
                input_event_history, input_test, layout_test, manual_layout_test,
                svg_test, window_resize_sub_windows, ResizeTestWindow};

use std::sync::Arc;

use agg_gui::{
    Color, DrawCtx, Event, EventResult,
    FlexColumn, Font, Insets, Label, MarkdownView,
    Rect, ScrollView, Size, SizedBox, Widget,
};
use agg_gui::widget::paint_subtree;

// ---------------------------------------------------------------------------
// "Coming Soon" placeholder
// ---------------------------------------------------------------------------

struct ComingSoon {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
}

impl ComingSoon {
    fn new() -> Self {
        Self { bounds: Rect::default(), children: Vec::new() }
    }
}

impl Widget for ComingSoon {
    fn type_name(&self) -> &'static str { "ComingSoon" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Returns a minimal placeholder window content for unimplemented demos.
pub fn coming_soon() -> Box<dyn Widget> {
    Box::new(ComingSoon::new())
}

// ---------------------------------------------------------------------------
// Logo widget — vector drawing of the agg-gui mark shown in the About window.
// Matches the SVG favicon at `demo/public/favicon.svg`.
// ---------------------------------------------------------------------------

struct LogoWidget {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    /// Bold lowercase "a" centered inside the window shape.
    letter:   Label,
    size:     f64,
}

impl LogoWidget {
    fn new(font: Arc<Font>, size: f64) -> Self {
        let letter = Label::new("a", font)
            .with_font_size(size * 0.55)
            .with_color(Color::rgb(1.0, 1.0, 1.0));
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            letter,
            size,
        }
    }
}

impl Widget for LogoWidget {
    fn type_name(&self) -> &'static str { "LogoWidget" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, _available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, self.size, self.size);
        let s = self.letter.layout(Size::new(self.size, self.size));
        self.letter.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        Size::new(self.size, self.size)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let s  = self.size;
        let r  = s * 0.16;           // corner radius
        let tb = s * 0.20;           // title-bar height
        let dr = s * 0.032;          // dot radius

        // Window body (accent blue).
        ctx.set_fill_color(Color::rgb(0.29, 0.56, 0.89));
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, s, s, r);
        ctx.fill();

        // Title bar (darker blue, anchored at the TOP of the logo — which in
        // this widget's Y-up local space means high Y).
        ctx.set_fill_color(Color::rgb(0.12, 0.31, 0.55));
        ctx.begin_path();
        ctx.rounded_rect(0.0, s - tb, s, tb, r);
        ctx.fill();
        // Square off the bottom of the title bar so only top corners are round.
        ctx.set_fill_color(Color::rgb(0.12, 0.31, 0.55));
        ctx.begin_path();
        ctx.rect(0.0, s - tb, s, r.min(tb));
        ctx.fill();

        // Three traffic-light dots, left-aligned in the title bar.
        let dots = [
            (Color::rgb(1.00, 0.37, 0.34), 0.10),
            (Color::rgb(1.00, 0.74, 0.18), 0.18),
            (Color::rgb(0.16, 0.78, 0.25), 0.26),
        ];
        let dot_y = s - tb * 0.5;
        for (col, fx) in &dots {
            ctx.set_fill_color(*col);
            ctx.begin_path();
            ctx.circle(s * *fx, dot_y, dr);
            ctx.fill();
        }

        // Centered "a" glyph in the body area (below the title bar).
        let body_top = s - tb;
        let lw = self.letter.bounds().width;
        let lh = self.letter.bounds().height;
        let lx = (s - lw) * 0.5;
        let ly = (body_top - lh) * 0.5;
        self.letter.set_bounds(Rect::new(lx, ly, lw, lh));

        ctx.save();
        ctx.translate(lx, ly);
        paint_subtree(&mut self.letter, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// About window
// ---------------------------------------------------------------------------

/// About window content: renders README.md via `MarkdownView` inside a scroll view.
pub fn about(font: Arc<Font>) -> Box<dyn Widget> {
    // Embed README.md at compile time.
    let readme = include_str!("../../README.md");

    // Base directory for resolving relative image paths (agg-gui workspace root).
    // On native targets we can load files at runtime; on WASM this will always
    // return None (files not accessible), showing placeholder boxes instead.
    let base_dir = {
        // CARGO_MANIFEST_DIR is set at compile time for demo-ui; its parent is agg-gui/.
        let manifest = env!("CARGO_MANIFEST_DIR");
        std::path::PathBuf::from(manifest)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    };

    let md_view = MarkdownView::new(readme, Arc::clone(&font))
        .with_font_size(13.0)
        .with_padding(12.0)
        .with_image_provider(move |url| {
            // Only handle relative paths / local file URLs on native.
            let path = if url.starts_with("http://") || url.starts_with("https://") {
                // Remote URL — not supported at runtime; return None for placeholder.
                return None;
            } else {
                base_dir.join(url)
            };
            load_png(&path)
        });

    // Logo hero at the top of the About window, above the README content.
    let logo = SizedBox::new()
        .with_width(96.0)
        .with_height(96.0)
        .with_margin(Insets::from_sides(0.0, 0.0, 16.0, 8.0))
        .with_child(Box::new(LogoWidget::new(Arc::clone(&font), 96.0)));

    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .with_padding(0.0);
    col.push(Box::new(logo), 0.0);
    col.push(Box::new(md_view), 0.0);

    Box::new(ScrollView::new(Box::new(col)))
}

// ---------------------------------------------------------------------------
// PNG loader (shared by about())
// ---------------------------------------------------------------------------

/// Decode a PNG file to raw RGBA8 pixel data (top-row first).
/// Returns `None` if the file doesn't exist or can't be decoded.
fn load_png(path: &std::path::Path) -> Option<(Vec<u8>, u32, u32)> {
    use std::io::BufReader;
    let file = std::fs::File::open(path).ok()?;
    let decoder = png::Decoder::new(BufReader::new(file));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let w = info.width;
    let h = info.height;
    // Convert to RGBA8 regardless of source format.
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => {
            let src = &buf[..info.buffer_size()];
            let mut out = Vec::with_capacity(w as usize * h as usize * 4);
            for chunk in src.chunks(3) {
                out.extend_from_slice(chunk);
                out.push(255);
            }
            out
        }
        png::ColorType::Grayscale => {
            let src = &buf[..info.buffer_size()];
            let mut out = Vec::with_capacity(w as usize * h as usize * 4);
            for &v in src {
                out.extend_from_slice(&[v, v, v, 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let src = &buf[..info.buffer_size()];
            let mut out = Vec::with_capacity(w as usize * h as usize * 4);
            for chunk in src.chunks(2) {
                out.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
            }
            out
        }
        // Indexed/other formats — show placeholder.
        _ => return None,
    };
    Some((rgba, w, h))
}

// ---------------------------------------------------------------------------
// 3D Animation window content
// ---------------------------------------------------------------------------

/// Wrap the platform-provided GL widget for placement inside a floating
/// `Window`.  No label, no decorative chrome — the GL widget fills the
/// window's content rect and paints its own theme-aware background each
/// frame (see `GlCubeWidget::paint`), so the whole content area follows
/// the active theme automatically when the user toggles light / dark.
pub fn cube_content(_font: Arc<Font>, cube_widget: Box<dyn Widget>) -> Box<dyn Widget> {
    cube_widget
}
