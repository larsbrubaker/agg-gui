//! Coordinate system invariant tests.
//!
//! These tests guard the first-quadrant (Y-up) invariant at the framebuffer
//! and GfxCtx layers. They run on every commit.

use crate::{
    App, Button, Color, ComboBox, CompOp, Container, FlexColumn, FlexRow, Framebuffer, GfxCtx, Key,
    Modifiers, MouseButton, ScrollBarColor, ScrollBarKind, ScrollBarStyle, ScrollView, Size,
    SizedBox, Splitter, TabView, TextField, ToggleSwitch, Widget,
};

/// Sample RGBA at pixel (x, y) in a framebuffer.
/// (x=0, y=0) is the bottom-left corner in Y-up space.
fn sample(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[idx], p[idx + 1], p[idx + 2], p[idx + 3]]
}

fn is_white(pixel: [u8; 4]) -> bool {
    pixel[0] > 200 && pixel[1] > 200 && pixel[2] > 200
}

fn is_red(pixel: [u8; 4]) -> bool {
    pixel[0] > 200 && pixel[1] < 50 && pixel[2] < 50
}

fn is_dark(pixel: [u8; 4]) -> bool {
    pixel[0] < 50 && pixel[1] < 50 && pixel[2] < 50
}

const TEST_FONT: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

// ---------------------------------------------------------------------------
// Phase 1 — coordinate system invariants
// ---------------------------------------------------------------------------

/// A point drawn at Y=10 in a 100×100 buffer must be near the BOTTOM of the
/// buffer (low row index), not the top. This verifies the Y-up invariant at
/// the framebuffer level.
mod drawing;
mod inspector_tree;
mod layout_lcd;
mod retained_layers;
mod touch_scroll;
mod tree_view;
mod widgets;
mod window_layout;
mod window_maximize;
mod windowing;
