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

mod animation;
mod basic;
mod code_example;
mod font_book;
mod frame_demo;
mod gallery;
mod interaction;
mod lion;
mod menu_demo;
mod misc;
mod mobile_keyboard;
mod screenshot_demo;
mod scrolling;
mod system;
mod system_fonts;
mod tests;
mod text_demos;
mod truetype_lcd;

// Re-export every public demo builder so callers use `windows::foo(font)`.
pub use animation::{bezier_curve, dancing_strings, painting};
pub use basic::{code_editor, sliders, text_edit, tooltips};
pub use code_example::code_example;
pub use font_book::font_book;
pub use frame_demo::frame_demo;
pub use gallery::widget_gallery;
pub use interaction::{drag_and_drop, panels_demo, popups_demo, scene_demo};
pub use lion::lion_demo;
pub use menu_demo::menu_demo;
pub use misc::{extra_viewport, highlighting, interactive_container, misc_demos};
pub use mobile_keyboard::{mobile_keyboard, TITLE as MOBILE_KEYBOARD_TITLE};
pub use screenshot_demo::screenshot_demo;
pub use scrolling::scrolling_demo;
pub use system::{
    cells as system_cells, init_cells as init_system_cells, system_view, SystemCells,
};
pub use system_fonts::{
    apply_font_by_index, default_font_index, font_asset_by_name, font_cache_epoch,
    font_option_index, font_option_names, install_font_bytes, load_font_by_name, loaded_item_fonts,
    request_all_font_previews, request_font_by_index, take_pending_font_request, FontAsset,
    DEFAULT_FONT_NAME, EMOJI_FONT_PATH, FONT_AWESOME_PATH,
};
pub use tests::{
    clipboard_test, cursor_test, grid_test, id_test, input_event_history, input_test, layout_test,
    manual_layout_test, svg_test, window_resize_sub_windows, ResizeTestWindow,
};
pub use text_demos::{
    modals_demo, multi_touch, strip_demo, table_demo, text_layout, undo_redo, window_options,
};
pub use truetype_lcd::truetype_lcd_view;

use std::sync::Arc;

use agg_gui::{DrawCtx, Event, EventResult, Font, MarkdownView, Rect, ScrollView, Size, Widget};

// ---------------------------------------------------------------------------
// "Coming Soon" placeholder
// ---------------------------------------------------------------------------

struct ComingSoon {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl ComingSoon {
    fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }
}

impl Widget for ComingSoon {
    fn type_name(&self) -> &'static str {
        "ComingSoon"
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
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
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
    let readme = about_readme_markdown(include_str!("../../README.md"));

    // Base directory for resolving relative image paths (agg-gui workspace root).
    // HTTP(S) images are resolved asynchronously by MarkdownView itself.
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
        .on_link_click(crate::url::open_url)
        .on_image_open(crate::url::open_url)
        .with_image_provider(move |url| {
            // Only handle relative paths / local file URLs on native.
            let path = if url.starts_with("http://") || url.starts_with("https://") {
                return None;
            } else {
                base_dir.join(url)
            };
            load_png(&path)
        });

    Box::new(ScrollView::new(Box::new(md_view)))
}

#[cfg(not(target_arch = "wasm32"))]
fn about_readme_markdown(readme: &'static str) -> String {
    readme.to_string()
}

#[cfg(target_arch = "wasm32")]
fn about_readme_markdown(readme: &'static str) -> String {
    strip_dynamic_readme_badges(readme)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn strip_dynamic_readme_badges(readme: &str) -> String {
    let mut stripped = String::with_capacity(readme.len());
    for line in readme.lines() {
        if is_dynamic_readme_badge_line(line) {
            continue;
        }
        stripped.push_str(line);
        stripped.push('\n');
    }
    stripped
}

#[cfg(any(test, target_arch = "wasm32"))]
fn is_dynamic_readme_badge_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("[![")
        && (trimmed.contains("img.shields.io/")
            || (trimmed.contains("docs.rs/") && trimmed.contains("/badge.svg"))
            || (trimmed.contains("github.com/") && trimmed.contains("/badge.svg")))
}

#[cfg(test)]
mod about_tests {
    use super::*;

    #[test]
    fn strip_dynamic_readme_badges_removes_remote_svg_badges() {
        let readme = "\
# agg-gui

[![crates.io](https://img.shields.io/crates/v/agg-gui.svg)](https://crates.io/crates/agg-gui)
[![docs.rs](https://docs.rs/agg-gui/badge.svg)](https://docs.rs/agg-gui)
[![CI](https://github.com/larsbrubaker/agg-gui/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/larsbrubaker/agg-gui/actions/workflows/ci.yml)

## Live Demo
";

        let stripped = strip_dynamic_readme_badges(readme);

        assert!(!stripped.contains("img.shields.io"));
        assert!(!stripped.contains("docs.rs/agg-gui/badge.svg"));
        assert!(!stripped.contains("actions/workflows/ci.yml/badge.svg"));
        assert!(stripped.contains("# agg-gui"));
        assert!(stripped.contains("## Live Demo"));
    }

    #[test]
    fn strip_dynamic_readme_badges_keeps_hero_image_and_links() {
        let readme = "\
> **[Open interactive WASM demo ->](https://larsbrubaker.github.io/agg-gui/)**

[![agg-gui demo](agg-gui/readme_hero.png)](https://larsbrubaker.github.io/agg-gui/)
";

        let stripped = strip_dynamic_readme_badges(readme);

        assert!(stripped.contains("Open interactive WASM demo"));
        assert!(stripped.contains("agg-gui/readme_hero.png"));
    }
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

/// Wrap the platform-provided cube widget for placement inside a floating
/// `Window`.  Stacks an MSAA segmented row above the cube — toggling it
/// re-builds the bar-grid renderer with the new sample count on the next
/// paint (no relaunch needed; SSAA is scoped to the bar-grid framebuffer).
///
/// The toolbar surfaces actual backbuffer MB.  Rather than plumb a
/// shared-cell signal back from the cube widget (which lives in a
/// different crate), we wrap the cube in a small probe widget here in
/// demo-ui that copies its own laid-out bounds into a `Cell<(u32, u32)>`
/// on every layout pass.  The toolbar reads the same cell — no cross-crate
/// API changes, no `WgpuCubeWidget::new` signature shift.
pub fn cube_content(
    font: Arc<Font>,
    cube_widget: Box<dyn Widget>,
    msaa_cell: std::rc::Rc<std::cell::Cell<u8>>,
) -> Box<dyn Widget> {
    use agg_gui::FlexColumn;

    let cube_pixel_size: std::rc::Rc<std::cell::Cell<(u32, u32)>> =
        std::rc::Rc::new(std::cell::Cell::new((0, 0)));
    let toolbar = Box::new(crate::backend_panel::SsaaRow::new(
        Arc::clone(&font),
        msaa_cell,
        crate::backend_panel::SsaaRow::CUBE_SEGMENTS,
        std::rc::Rc::clone(&cube_pixel_size),
    )) as Box<dyn Widget>;

    let probe = Box::new(CubeSizeProbe::new(cube_widget, cube_pixel_size)) as Box<dyn Widget>;

    Box::new(
        FlexColumn::new()
            .with_gap(0.0)
            .add(toolbar)
            .add_flex(probe, 1.0),
    )
}

/// One-child wrapper that snapshots its inner widget's laid-out bounds
/// (in logical pixels) into a shared cell after every `layout()` pass.
/// Used by `cube_content` so the SSAA toolbar can show the actual cube
/// rect alongside the memory multiplier — no shared state added to
/// `WgpuCubeWidget` itself.  Paint / events forward through the
/// framework's child traversal: `inner` lives in `self.children[0]`.
struct CubeSizeProbe {
    bounds: agg_gui::Rect,
    children: Vec<Box<dyn Widget>>,
    size: std::rc::Rc<std::cell::Cell<(u32, u32)>>,
}

impl CubeSizeProbe {
    fn new(inner: Box<dyn Widget>, size: std::rc::Rc<std::cell::Cell<(u32, u32)>>) -> Self {
        Self {
            bounds: agg_gui::Rect::default(),
            children: vec![inner],
            size,
        }
    }
}

impl Widget for CubeSizeProbe {
    fn type_name(&self) -> &'static str {
        "CubeSizeProbe"
    }
    fn bounds(&self) -> agg_gui::Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: agg_gui::Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: agg_gui::Size) -> agg_gui::Size {
        if let Some(child) = self.children.first_mut() {
            let size = child.layout(available);
            child.set_bounds(agg_gui::Rect::new(0.0, 0.0, size.width, size.height));
            self.size
                .set((size.width.round() as u32, size.height.round() as u32));
            self.bounds = agg_gui::Rect::new(0.0, 0.0, size.width, size.height);
            size
        } else {
            self.bounds = agg_gui::Rect::new(0.0, 0.0, available.width, available.height);
            available
        }
    }

    fn paint(&mut self, _ctx: &mut dyn agg_gui::DrawCtx) {
        // Inner widget paints itself through the framework's child traversal.
    }

    fn on_event(&mut self, _event: &agg_gui::Event) -> agg_gui::EventResult {
        agg_gui::EventResult::Ignored
    }
}
