//! Shared demo UI — identical widget tree for both native and WASM targets.

mod api;
mod app_builder;
#[cfg(test)]
mod app_builder_tests;
mod backend_panel;
mod content;
mod font_init;
mod font_picker;
mod rendering_test;
mod shell;
mod sidebar;
mod specs;
mod state;
mod top_bar;
mod url;
mod windows;

pub use api::{DemoHandles, PlatformHooks, PlatformKind};
pub use app_builder::build_demo_ui;
pub use backend_panel::{FrameHistory, RunMode};
pub use state::{SavedState, StateAccessor, WindowState};
pub use windows::{
    font_asset_by_name, install_font_bytes, load_font_by_name, take_pending_font_request,
    window_resize_sub_windows, FontAsset, ResizeTestWindow, DEFAULT_FONT_NAME, EMOJI_FONT_PATH,
    FONT_AWESOME_PATH,
};

/// Encode a top-down RGBA8 buffer as a PNG.
pub fn encode_png_rgba(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    agg_gui::screenshot::encode_png_rgba(rgba, width, height).unwrap_or_else(|e| {
        eprintln!("encode_png_rgba failed: {e}");
        Vec::new()
    })
}
