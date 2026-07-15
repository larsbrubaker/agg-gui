//! Basic diagnostic test windows.

mod controls;
mod layout;

pub use controls::{clipboard_test, cursor_test, grid_test, input_event_history};
pub use layout::{input_test, layout_test, manual_layout_test};
