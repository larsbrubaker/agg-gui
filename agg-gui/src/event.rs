//! Event types for the widget system.
//!
//! All coordinates in events are **first-quadrant (Y-up)** by the time any
//! widget code sees them. The single Y-down → Y-up conversion happens at the
//! platform boundary inside [`crate::widget::App`].

use crate::geometry::Point;
use crate::touch_state::MultiTouchInfo;

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
    /// Page-up / page-down — scroll the caret by one viewport height of lines
    /// in a multiline editor.
    PageUp,
    PageDown,
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
    /// One or more files were dropped onto the window at `pos`, delivered
    /// as their contents rather than as paths.
    ///
    /// The browser counterpart of [`Event::FileDropped`]: a web page never
    /// sees a dropped file's path, only its name and bytes, so the web shell
    /// reads every dropped file and sends them together in one event once
    /// all have been read. Native shells keep sending `FileDropped` (paths).
    /// An app that runs in both places handles both variants. `files` is
    /// non-empty; routing and coordinates are exactly those of
    /// `FileDropped`.
    FileDataDropped {
        pos: Point,
        files: Vec<DroppedFileData>,
    },
    /// A file drag from outside the app is over the window at `pos` (it
    /// entered, or moved).
    ///
    /// Lets a widget show drop-target feedback or a live preview before the
    /// drop. Routed like a drop: to the widget under `pos`, bubbling, then
    /// offered to the rest of the tree if nothing on that path consumed it.
    /// The drag ends with [`Event::FileDragLeave`].
    ///
    /// `paths` lists the dragged files where the platform reveals them
    /// (native: winit reports each hovered path). It is **empty in the
    /// browser**, which hides file names and contents until the drop.
    ///
    /// How often it arrives depends on the platform: the browser sends one
    /// per `dragover` (continuously as the pointer moves); native shells
    /// send one per hovered file when the drag enters, then again on every
    /// cursor move the OS reports during the drag — on Windows and macOS
    /// winit reports none, so the position is the entry point.
    FileDragHover {
        pos: Point,
        paths: Vec<std::path::PathBuf>,
    },
    /// The file drag that sent [`Event::FileDragHover`] is over: it left the
    /// window, was cancelled, or ended in a drop. Delivered to **every**
    /// widget in the tree (it does not bubble and cannot be consumed), so
    /// any widget showing drag feedback can clear it. On a drop it arrives
    /// before the `FileDropped` / `FileDataDropped` event.
    FileDragLeave,
    /// A two-or-more-finger touch gesture is active this frame.
    ///
    /// Routed like a captured pointer: on the frame the gesture begins,
    /// the framework hit-tests [`MultiTouchInfo::center_pos`] down the
    /// tree and delivers to the deepest widget that consumes it; that
    /// widget then receives every subsequent frame's aggregate until the
    /// gesture ends, even if the centroid drifts outside its bounds
    /// (standard capture semantics). Widgets accumulate the per-frame
    /// `zoom_delta` / `rotation_delta` / `translation_delta` into their
    /// own transform state.
    ///
    /// `info.center_pos` is translated into the receiving widget's local
    /// Y-up space as the event descends, exactly like a mouse `pos`. The
    /// deltas are displacement vectors and are **not** translated — they
    /// are the same in every coordinate frame.
    MultiTouch { info: MultiTouchInfo },
    /// The set of held modifier keys changed with no other key involved
    /// (e.g. Shift pressed or released mid-drag).
    ///
    /// Sent by [`crate::widget::App::on_modifiers_changed`] to the widget
    /// holding mouse capture (an in-progress drag), and to the focused
    /// widget when that is a different widget. Widgets that don't care can
    /// ignore it. Pointer events that predate this variant
    /// (`MouseMove`) don't carry modifiers; read [`current_modifiers`]
    /// while handling them instead.
    ModifiersChanged { modifiers: Modifiers },
    /// The pointer entered this widget's bounds: it is now on the hovered
    /// chain (the widget under the pointer, or one of its ancestors).
    ///
    /// Sent by [`crate::widget::App`] to **every** widget that joins the
    /// chain, shallowest first, before the `MouseMove` that caused it is
    /// dispatched — whichever descendant then handles that move. This is
    /// agg-sharp's `MouseEnterBounds`: a composite (a field wrapper, a tab
    /// strip) learns the pointer arrived even though a child takes the
    /// moves. Delivered straight to the widget (it does not bubble); the
    /// result only decides whether a repaint is scheduled. While a widget
    /// holds pointer capture its ancestors stay on the chain and only the
    /// captured widget itself leaves and re-enters as the pointer crosses
    /// it; everything else is settled when capture ends.
    MouseEnter,
    /// The pointer left this widget's bounds (it is no longer on the
    /// hovered chain). agg-sharp's `MouseLeaveBounds`. Sent deepest first,
    /// before the `MouseMove` that caused it; see [`Event::MouseEnter`].
    MouseLeave,
}

/// One file of an [`Event::FileDataDropped`]: its name (no directory — the
/// browser never reveals one) and its contents.
///
/// The bytes are reference-counted so the event can be cloned as it descends
/// the widget tree without copying file contents.
#[derive(Clone, PartialEq, Eq)]
pub struct DroppedFileData {
    /// The file name as the platform reports it, e.g. `"part.stl"`.
    pub name: String,
    /// The whole file.
    pub bytes: std::sync::Arc<[u8]>,
}

impl DroppedFileData {
    pub fn new(name: impl Into<String>, bytes: impl Into<std::sync::Arc<[u8]>>) -> Self {
        Self {
            name: name.into(),
            bytes: bytes.into(),
        }
    }
}

impl std::fmt::Debug for DroppedFileData {
    // The contents can be megabytes; the length is what a log needs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DroppedFileData")
            .field("name", &self.name)
            .field("len", &self.bytes.len())
            .finish()
    }
}

thread_local! {
    static CURRENT_MODIFIERS: std::cell::Cell<Modifiers> =
        std::cell::Cell::new(Modifiers::default());
}

/// The keyboard modifiers held as of the most recent input event the
/// [`crate::widget::App`] processed.
///
/// `Event::MouseMove` has no `modifiers` field (adding one would break every
/// `MouseMove { pos }` pattern in downstream apps), so a widget that needs
/// Shift/Ctrl during hover or a drag reads this while handling the move.
/// The App updates it on every modifier-carrying input (mouse down/up, key
/// down/up, wheel) and on [`crate::widget::App::on_modifiers_changed`],
/// which platform shells call whenever the OS reports a modifier change.
pub fn current_modifiers() -> Modifiers {
    CURRENT_MODIFIERS.with(|m| m.get())
}

/// Record the current modifier state. Returns `true` when it changed.
/// Called by the App's input entry points; widgets should not need it.
pub(crate) fn set_current_modifiers(mods: Modifiers) -> bool {
    CURRENT_MODIFIERS.with(|m| m.replace(mods) != mods)
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
