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
mod misc;
mod interaction;
mod text_demos;
mod tests;

// Re-export every public demo builder so callers use `windows::foo(font)`.
pub use gallery::widget_gallery;
pub use basic::{sliders, text_edit, tooltips, code_editor};
pub use code_example::code_example;
pub use animation::{bezier_curve, dancing_strings, painting};
pub use misc::{frame_demo, extra_viewport, highlighting, interactive_container,
               font_book, misc_demos};
pub use interaction::{drag_and_drop, scrolling_demo, panels_demo, popups_demo,
                      scene_demo, screenshot_demo};
pub use text_demos::{strip_demo, table_demo, text_layout, undo_redo,
                     window_options, modals_demo, multi_touch};
pub use tests::{clipboard_test, cursor_test, grid_test, id_test,
                input_event_history, input_test, layout_test, manual_layout_test,
                svg_test, tessellation_test, window_resize_test};

use std::sync::Arc;

use agg_gui::{
    Color, DrawCtx, Event, EventResult,
    FlexColumn, Font, Label, MarkdownView,
    Rect, ScrollView, Size, Widget,
};

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

    Box::new(ScrollView::new(Box::new(md_view)))
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
// 3D Cube window content
// ---------------------------------------------------------------------------

/// Wrap the platform-provided GL cube widget in a dark-themed column with a
/// label, ready to be placed inside a floating `Window`.
pub fn cube_content(font: Arc<Font>, cube_widget: Box<dyn Widget>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(10.0)
        .with_background(Color::rgb(0.08, 0.08, 0.12));

    col.push(Box::new(Label::new("GL — rotating cube", Arc::clone(&font))
        .with_font_size(11.0).with_color(Color::rgba(1.0, 1.0, 1.0, 0.55))), 0.0);
    col.push(cube_widget, 1.0);

    Box::new(col)
}
