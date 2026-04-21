//! Bridge between `winit` types and this crate's input/cursor types.
//!
//! Enabled with the `winit-adapter` feature.  Host crates that use
//! winit for window/event handling forward raw `winit` events through
//! these helpers instead of re-implementing the mapping themselves.

use winit::event::MouseButton as WinitMouseButton;
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon as WinitCursor, Window as WinitWindow};

use crate::cursor::CursorIcon;
use crate::event::{Key, Modifiers, MouseButton};

/// Map a winit [`MouseButton`](WinitMouseButton) to this crate's
/// [`MouseButton`].  Unrecognised variants return `Other(255)`.
pub fn mouse_button(b: WinitMouseButton) -> MouseButton {
    match b {
        WinitMouseButton::Left     => MouseButton::Left,
        WinitMouseButton::Right    => MouseButton::Right,
        WinitMouseButton::Middle   => MouseButton::Middle,
        WinitMouseButton::Other(n) => MouseButton::Other(n as u8),
        _                          => MouseButton::Other(255),
    }
}

/// Map a winit [`ModifiersState`] to this crate's [`Modifiers`].
pub fn modifiers(s: ModifiersState) -> Modifiers {
    Modifiers {
        shift: s.shift_key(),
        ctrl:  s.control_key(),
        alt:   s.alt_key(),
        // winit's "super" is the platform "command" key: Cmd (macOS),
        // Windows key (Windows), Super (X11).
        meta:  s.super_key(),
    }
}

/// Map a winit logical key to this crate's [`Key`].  Returns `None` for
/// keys that don't have a direct equivalent (e.g. F-keys, media keys —
/// hosts can still inspect the raw winit event if they need them).
pub fn key(k: &WinitKey) -> Option<Key> {
    Some(match k {
        WinitKey::Named(NamedKey::ArrowUp)    => Key::ArrowUp,
        WinitKey::Named(NamedKey::ArrowDown)  => Key::ArrowDown,
        WinitKey::Named(NamedKey::ArrowLeft)  => Key::ArrowLeft,
        WinitKey::Named(NamedKey::ArrowRight) => Key::ArrowRight,
        WinitKey::Named(NamedKey::Enter)      => Key::Enter,
        WinitKey::Named(NamedKey::Space)      => Key::Char(' '),
        WinitKey::Named(NamedKey::Tab)        => Key::Tab,
        WinitKey::Named(NamedKey::Escape)     => Key::Escape,
        WinitKey::Named(NamedKey::Backspace)  => Key::Backspace,
        WinitKey::Named(NamedKey::Home)       => Key::Home,
        WinitKey::Named(NamedKey::End)        => Key::End,
        WinitKey::Named(NamedKey::Delete)     => Key::Delete,
        WinitKey::Named(NamedKey::Insert)     => Key::Insert,
        WinitKey::Named(NamedKey::PageUp)     => Key::Other("PageUp".into()),
        WinitKey::Named(NamedKey::PageDown)   => Key::Other("PageDown".into()),
        WinitKey::Character(s)                => Key::Char(s.chars().next()?),
        _                                     => return None,
    })
}

/// Translate a [`CursorIcon`] to winit's [`CursorIcon`](WinitCursor).
/// `CursorIcon::None` falls back to `Default` — callers who want the
/// cursor actually hidden should use [`apply_cursor`] which handles the
/// `set_cursor_visible(false)` case.
pub fn cursor_icon(icon: CursorIcon) -> WinitCursor {
    match icon {
        CursorIcon::Default          => WinitCursor::Default,
        CursorIcon::None             => WinitCursor::Default,
        CursorIcon::ContextMenu      => WinitCursor::ContextMenu,
        CursorIcon::Help             => WinitCursor::Help,
        CursorIcon::PointingHand     => WinitCursor::Pointer,
        CursorIcon::Progress         => WinitCursor::Progress,
        CursorIcon::Wait             => WinitCursor::Wait,
        CursorIcon::Cell             => WinitCursor::Cell,
        CursorIcon::Crosshair        => WinitCursor::Crosshair,
        CursorIcon::Text             => WinitCursor::Text,
        CursorIcon::VerticalText     => WinitCursor::VerticalText,
        CursorIcon::Alias            => WinitCursor::Alias,
        CursorIcon::Copy             => WinitCursor::Copy,
        CursorIcon::Move             => WinitCursor::Move,
        CursorIcon::NoDrop           => WinitCursor::NoDrop,
        CursorIcon::NotAllowed       => WinitCursor::NotAllowed,
        CursorIcon::Grab             => WinitCursor::Grab,
        CursorIcon::Grabbing         => WinitCursor::Grabbing,
        CursorIcon::AllScroll        => WinitCursor::AllScroll,
        CursorIcon::ResizeHorizontal => WinitCursor::EwResize,
        CursorIcon::ResizeNeSw       => WinitCursor::NeswResize,
        CursorIcon::ResizeNwSe       => WinitCursor::NwseResize,
        CursorIcon::ResizeVertical   => WinitCursor::NsResize,
        CursorIcon::ResizeEast       => WinitCursor::EResize,
        CursorIcon::ResizeSouthEast  => WinitCursor::SeResize,
        CursorIcon::ResizeSouth      => WinitCursor::SResize,
        CursorIcon::ResizeSouthWest  => WinitCursor::SwResize,
        CursorIcon::ResizeWest       => WinitCursor::WResize,
        CursorIcon::ResizeNorthWest  => WinitCursor::NwResize,
        CursorIcon::ResizeNorth      => WinitCursor::NResize,
        CursorIcon::ResizeNorthEast  => WinitCursor::NeResize,
        CursorIcon::ResizeColumn     => WinitCursor::ColResize,
        CursorIcon::ResizeRow        => WinitCursor::RowResize,
        CursorIcon::ZoomIn           => WinitCursor::ZoomIn,
        CursorIcon::ZoomOut          => WinitCursor::ZoomOut,
    }
}

/// Apply a [`CursorIcon`] to a winit window, handling cursor hiding.
/// Call after forwarding a mouse event to [`App`](crate::App) (which
/// updates the thread-local via `cursor::reset_cursor_icon` + widget
/// dispatch) so the hover target's preferred cursor is reflected.
pub fn apply_cursor(window: &WinitWindow, icon: CursorIcon) {
    if icon == CursorIcon::None {
        window.set_cursor_visible(false);
    } else {
        window.set_cursor_visible(true);
        window.set_cursor(cursor_icon(icon));
    }
}
