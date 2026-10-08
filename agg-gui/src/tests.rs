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

mod absolute_layout;
mod async_wakeup_paint;
mod backbuffer_scale;
mod button_click_focus;
mod button_click_semantics;
mod capture_reorder;
mod caret_deadline;
mod clip_path_software;
mod color_clickaway;
mod color_dialog_overlay;
mod color_wheel_picker;
mod default_action;
/// A point drawn at Y=10 in a 100×100 buffer must be near the BOTTOM of the
/// buffer (low row index), not the top. This verifies the Y-up invariant at
/// the framebuffer level.
mod draw_report;
mod drawing;
mod ellipse_path;
mod event_pointer;
mod fitted_layout;
mod flex_gap;
mod focus;
mod focus_blur;
mod focus_replaced_subtree;
mod hover_enter_leave;
mod image_icons;
mod inspector_hover;
mod inspector_tree;
mod keyboard_lift;
mod label_ellipsis;
mod label_hidpi_backbuffer;
mod label_theme;
mod layer_compositing;
mod layout_lcd;
mod layout_request;
mod lcd_backbuffer_collapse;
mod menu_hidpi_scale;
mod multi_touch_routing;
mod on_screen_keyboard;
pub(crate) mod paint_recorder;
mod platform_multi_click;
mod pointer_modifiers;
mod radio_button;
mod rasterizer_clip;
#[cfg(feature = "reflect")]
mod reflect_roundtrip;
mod reserve_inset;
mod retained_layers;
mod rich_toolbar_color_overlay;
mod scene_focus;
mod scroll_view;
mod shell_input;
mod stack_aligned;
mod text_defaults;
mod text_field_select_all;
mod tooltip_window_hover;
mod touch_scroll;
mod trackpad_pinch;
mod trackpad_pinch_fingers;
mod tree_view;
mod under_mouse_state;
mod widget_cursors;
mod widget_enabled;
mod widget_names;
mod widgets;
mod window_layout;
mod window_maximize;
mod window_snap_coords;
mod windowing;
