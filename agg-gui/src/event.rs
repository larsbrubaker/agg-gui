//! Event types for the widget system.
//!
//! All coordinates in events are **first-quadrant (Y-up)** by the time any
//! widget code sees them. The single Y-down → Y-up conversion happens at the
//! platform boundary inside [`crate::widget::App`].

use crate::geometry::Point;

/// Which mouse button triggered a `MouseDown` or `MouseUp` event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Other(u8),
}

/// Modifier keys held at the time of an event.
///
/// `meta` is the platform-specific "super" key: **Cmd** on macOS, **Super /
/// Windows key** on Linux, **Windows key** on Windows. Widgets that want
/// portable command shortcuts should use the runtime platform helpers rather
/// than treating `ctrl || meta` as universally equivalent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

/// A logical keyboard key.
#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    /// A printable character, already translated through the keyboard layout.
    Char(char),
    Backspace,
    Delete,
    /// The `Insert` key.  Paired with `Shift`/`Ctrl` for classic Windows
    /// clipboard shortcuts (`Shift+Ins` paste, `Ctrl+Ins` copy).
    Insert,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    Tab,
    Enter,
    Escape,
    /// Any key not in the above set — not usually handled, included for
    /// completeness.
    Other(String),
}

/// A GUI event delivered to a widget.
///
/// Coordinate positions are in the **local** coordinate space of the widget
/// receiving the event (bottom-left origin, Y-up). The framework translates
/// positions as it descends the widget tree.
#[derive(Clone, Debug)]
pub enum Event {
    /// The cursor moved to `pos` (may be outside widget bounds — used to
    /// clear hover state).
    MouseMove { pos: Point },
    /// A mouse button was pressed at `pos`.
    MouseDown {
        pos: Point,
        button: MouseButton,
        modifiers: Modifiers,
    },
    /// A mouse button was released at `pos`.
    MouseUp {
        pos: Point,
        button: MouseButton,
        modifiers: Modifiers,
    },
    /// A key was pressed while this widget (or a descendant) had focus.
    KeyDown { key: Key, modifiers: Modifiers },
    /// A key was released.
    KeyUp { key: Key, modifiers: Modifiers },
    /// Sent by the framework when this widget gains keyboard focus.
    FocusGained,
    /// Sent by the framework when this widget loses keyboard focus.
    FocusLost,
    /// Mouse wheel scrolled.  `delta_y` is in logical pixels; positive =
    /// scroll up (content moves up, typical "natural" scroll direction).
    /// `delta_x` is horizontal wheel / trackpad input in the same units;
    /// positive = content moves right.  `pos` is the cursor location at the
    /// time of the scroll.
    MouseWheel {
        pos: Point,
        delta_y: f64,
        delta_x: f64,
        modifiers: Modifiers,
    },
}

/// What a widget returns from [`crate::widget::Widget::on_event`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventResult {
    /// The widget handled the event; stop propagation.
    Consumed,
    /// The widget did not handle the event; continue bubbling up.
    Ignored,
}
