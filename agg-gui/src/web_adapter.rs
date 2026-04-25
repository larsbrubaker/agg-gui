//! Bridge between web/JS event payloads and this crate's input types.
//!
//! Compiled only on `wasm32` targets.  Host crates forward DOM
//! KeyboardEvent payloads through [`key`] and use [`apply_cursor_to_css`]
//! to set the canvas's cursor from a [`CursorIcon`].

use crate::cursor::CursorIcon;
use crate::event::Key;

/// Parse a DOM `KeyboardEvent.key` string into this crate's [`Key`].
///
/// Named keys (`"Enter"`, `"ArrowLeft"`, …) map to their enum variant.
/// A single character becomes `Key::Char(c)`.  Anything else — e.g.
/// `"F5"`, `"MediaPlayPause"` — round-trips as `Key::Other(name)` so
/// hosts can still inspect it.
pub fn key(name: &str) -> Option<Key> {
    Some(match name {
        "Backspace" => Key::Backspace,
        "Delete" => Key::Delete,
        "Insert" => Key::Insert,
        "ArrowLeft" => Key::ArrowLeft,
        "ArrowRight" => Key::ArrowRight,
        "ArrowUp" => Key::ArrowUp,
        "ArrowDown" => Key::ArrowDown,
        "Home" => Key::Home,
        "End" => Key::End,
        "Tab" => Key::Tab,
        "Enter" => Key::Enter,
        "Escape" => Key::Escape,
        " " => Key::Char(' '),
        s if s.chars().count() == 1 => Key::Char(s.chars().next()?),
        s => Key::Other(s.to_string()),
    })
}

/// Produce the CSS `style` attribute value for applying a [`CursorIcon`]
/// to a DOM element — `"cursor:<name>"`.  Callers combine this with their
/// existing style string if they set other properties.
pub fn cursor_style(icon: CursorIcon) -> String {
    format!("cursor:{}", icon.to_css())
}
