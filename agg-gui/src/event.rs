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
    /// Mouse wheel scrolled.  Convention matches `winit` /
    /// `WheelEvent` after the OS applies its natural-scroll
    /// preference: **positive `delta_y` means the user wants to see
    /// content ABOVE the current view** (wheel rotated forward on
    /// Windows / wheel forward + natural-scroll on macOS).  Scroll
    /// containers should DECREASE their scroll offset when `delta_y`
    /// is positive.  `delta_x` follows the same sign rule for
    /// horizontal scroll (positive = see content to the LEFT).
    /// Magnitude is in logical pixels; line deltas should be
    /// pre-scaled by the platform shell (~40 px per line).
    MouseWheel {
        pos: Point,
        delta_y: f64,
        delta_x: f64,
        modifiers: Modifiers,
    },
    /// One or more files were dropped onto the window at `pos`.
    ///
    /// `paths` is non-empty. Native windowing layers (winit) typically
    /// emit one path per `WindowEvent::DroppedFile` — the framework
    /// either forwards each as its own `FileDropped` event, or batches
    /// drops within a single gesture into one event. Receivers should
    /// not rely on batching behaviour: handle each path in the vec.
    ///
    /// Coordinates follow the same convention as `MouseMove`/`MouseDown`:
    /// widget-local Y-up. The cursor lives at `pos` at the moment of
    /// drop, so widgets can spawn objects under the user's intent.
    FileDropped {
        pos: Point,
        paths: Vec<std::path::PathBuf>,
    },
}

/// What a widget returns from [`crate::widget::Widget::on_event`].
///
/// # Automatic invalidation
///
/// The framework's event dispatcher (see [`crate::widget::tree`]) treats a
/// [`Consumed`](EventResult::Consumed) result as "this widget changed
/// something visible" and schedules a repaint via
/// [`crate::animation::request_draw`] on the widget's behalf.  This makes the
/// correct default automatic: the most common framework bug is a new widget
/// that mutates paint-affecting state on an event but forgets to request a
/// draw, so parts of it don't repaint.  With auto-invalidation a plain
/// `Consumed` is always safe.
///
/// A widget that consumes a *high-frequency* event (typically `MouseMove`)
/// **without** any visual change — for example, a hover affordance that only
/// updates the OS cursor — should return
/// [`ConsumedQuiet`](EventResult::ConsumedQuiet) so it doesn't schedule a
/// wasteful repaint on every event.  `ConsumedQuiet` still stops propagation
/// exactly like `Consumed`; it only suppresses the automatic draw request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventResult {
    /// The widget handled the event and may have changed its appearance;
    /// stop propagation and schedule a repaint automatically.
    Consumed,
    /// The widget handled the event (stop propagation) but produced **no**
    /// visual change, so the dispatcher must NOT schedule a repaint.  Use
    /// this only for genuinely quiet consumption of high-frequency events.
    ConsumedQuiet,
    /// The widget did not handle the event; continue bubbling up.
    Ignored,
}

impl EventResult {
    /// `true` for both [`Consumed`](EventResult::Consumed) and
    /// [`ConsumedQuiet`](EventResult::ConsumedQuiet).
    ///
    /// Propagation, capture, and focus logic should branch on this rather
    /// than comparing against `Consumed` directly, so a quietly-consuming
    /// widget still stops the event exactly like a loud one.
    pub const fn is_consumed(self) -> bool {
        matches!(self, EventResult::Consumed | EventResult::ConsumedQuiet)
    }

    /// `true` only for [`Consumed`](EventResult::Consumed) — i.e. the
    /// dispatcher should call [`crate::animation::request_draw`] for this
    /// result.  `ConsumedQuiet` and `Ignored` return `false`.
    pub const fn requests_redraw(self) -> bool {
        matches!(self, EventResult::Consumed)
    }
}
