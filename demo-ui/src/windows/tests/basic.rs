//! Basic diagnostic test windows.

mod absolute_place;
mod controls;
mod input_probe;
mod layout;

pub use controls::{clipboard_test, cursor_test, input_event_history};
pub use input_probe::input_test;
pub use layout::{layout_test, manual_layout_test};
