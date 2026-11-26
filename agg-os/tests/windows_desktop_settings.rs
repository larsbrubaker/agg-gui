//! `agg_os::windows::desktop_settings`: every reader returns without
//! panicking, whatever the desktop's settings are, and the double-click time
//! is set. Only what holds on any Windows machine, a headless CI runner
//! included, is asserted; nothing here changes a setting.

#![cfg(all(windows, feature = "desktop-settings"))]

use agg_os::windows::desktop_settings::{
    cursor_pos, double_click_rect_px, double_click_time_ms, font_smoothing_enabled,
    font_smoothing_orientation, font_smoothing_type, is_remote_session, mouse_hover_time_ms,
    primary_display_rotated_quarter_turn,
};

#[test]
fn the_double_click_time_is_set() {
    assert_ne!(double_click_time_ms(), 0);
}

#[test]
fn every_reader_returns() {
    let _ = mouse_hover_time_ms();
    let _ = double_click_rect_px();
    let _ = cursor_pos();
    let _ = font_smoothing_enabled();
    let _ = font_smoothing_type();
    let _ = font_smoothing_orientation();
    let _ = is_remote_session();
    let _ = primary_display_rotated_quarter_turn();
}
