//! Test window implementations for egui-inspired diagnostic windows.
//!
//! Public facade for the split test-window modules.

mod basic;
mod resize;
mod svg;

pub use basic::{
    clipboard_test, cursor_test, grid_test, input_event_history, input_test, layout_test,
    manual_layout_test,
};
pub use resize::{window_resize_sub_windows, ResizeTestWindow};
pub use svg::svg_test;
