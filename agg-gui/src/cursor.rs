//! Cursor icon type and global cursor state.
//!
//! Widgets call [`set_cursor_icon`] in their `on_event` handler when hovered.
//! The platform harness calls [`current_cursor_icon`] after each mouse-move
//! dispatch and applies it to the OS window or browser canvas.
//!
//! The framework resets the cursor to [`CursorIcon::Default`] before each
//! mouse-move dispatch so the deepest hovered widget always wins without any
//! explicit reset in widget code.
//!
//! A cursor changed by code while the mouse rests goes through
//! [`invalidate_cursor`] (re-resolved by `App::refresh_cursor` on the next
//! paint), and shells push the result to the OS through [`WindowCursor`],
//! which only reports real changes.

use std::cell::Cell;

/// Logical cursor shape — mirrors egui's `CursorIcon` for portability.
///
/// Variants map 1-to-1 to CSS cursor names and to winit's `CursorIcon`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorIcon {
    /// Normal OS arrow cursor.
    #[default]
    Default,
    /// Hide the cursor entirely.
    None,
    /// A context menu is available (e.g. right-click target).
    ContextMenu,
    /// Question mark — hover for help.
    Help,
    /// Pointing hand, used for links and clickable items.
    PointingHand,
    /// Processing in progress, but the app is still interactive.
    Progress,
    /// Not yet ready — try later.
    Wait,
    /// Hover a cell in a table.
    Cell,
    /// For precision work (e.g. image editors).
    Crosshair,
    /// Text insertion caret.
    Text,
    /// Vertical text insertion caret.
    VerticalText,
    /// Alias / shortcut.
    Alias,
    /// Indicates that a copy will be made.
    Copy,
    /// Omnidirectional move.
    Move,
    /// Cannot drop here.
    NoDrop,
    /// Forbidden / not allowed.
    NotAllowed,
    /// The item under the cursor can be grabbed.
    Grab,
    /// Currently grabbing the item.
    Grabbing,
    /// Can scroll in any direction.
    AllScroll,
    /// Horizontal resize (left ↔ right).
    ResizeHorizontal,
    /// Diagonal resize `/` (NE ↔ SW).
    ResizeNeSw,
    /// Diagonal resize `\` (NW ↔ SE).
    ResizeNwSe,
    /// Vertical resize (up ↕ down).
    ResizeVertical,
    /// Resize rightwards.
    ResizeEast,
    /// Resize down-right.
    ResizeSouthEast,
    /// Resize downwards.
    ResizeSouth,
    /// Resize down-left.
    ResizeSouthWest,
    /// Resize leftwards.
    ResizeWest,
    /// Resize up-left.
    ResizeNorthWest,
    /// Resize upwards.
    ResizeNorth,
    /// Resize up-right.
    ResizeNorthEast,
    /// Resize a column.
    ResizeColumn,
    /// Resize a row.
    ResizeRow,
    /// Zoom in.
    ZoomIn,
    /// Zoom out.
    ZoomOut,
}

impl CursorIcon {
    /// CSS cursor value string for this icon (used by the WASM platform layer).
    pub fn to_css(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::None => "none",
            Self::ContextMenu => "context-menu",
            Self::Help => "help",
            Self::PointingHand => "pointer",
            Self::Progress => "progress",
            Self::Wait => "wait",
            Self::Cell => "cell",
            Self::Crosshair => "crosshair",
            Self::Text => "text",
            Self::VerticalText => "vertical-text",
            Self::Alias => "alias",
            Self::Copy => "copy",
            Self::Move => "move",
            Self::NoDrop => "no-drop",
            Self::NotAllowed => "not-allowed",
            Self::Grab => "grab",
            Self::Grabbing => "grabbing",
            Self::AllScroll => "all-scroll",
            Self::ResizeHorizontal => "ew-resize",
            Self::ResizeNeSw => "nesw-resize",
            Self::ResizeNwSe => "nwse-resize",
            Self::ResizeVertical => "ns-resize",
            Self::ResizeEast => "e-resize",
            Self::ResizeSouthEast => "se-resize",
            Self::ResizeSouth => "s-resize",
            Self::ResizeSouthWest => "sw-resize",
            Self::ResizeWest => "w-resize",
            Self::ResizeNorthWest => "nw-resize",
            Self::ResizeNorth => "n-resize",
            Self::ResizeNorthEast => "ne-resize",
            Self::ResizeColumn => "col-resize",
            Self::ResizeRow => "row-resize",
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
        }
    }
}

thread_local! {
    static CURSOR_ICON: Cell<CursorIcon> = const { Cell::new(CursorIcon::Default) };
}

/// Set the cursor icon for this frame.
///
/// Widgets call this in their [`MouseMove`][crate::Event::MouseMove] handler.
/// The cursor is automatically reset to [`CursorIcon::Default`] before each
/// mouse-move dispatch.
pub fn set_cursor_icon(icon: CursorIcon) {
    CURSOR_ICON.with(|c| c.set(icon));
}

/// Read the cursor icon set by widgets during the current frame.
///
/// Called by the platform harness after each `on_mouse_move` dispatch.
pub fn current_cursor_icon() -> CursorIcon {
    CURSOR_ICON.with(|c| c.get())
}

/// Reset to [`CursorIcon::Default`].
///
/// Called by the framework before each mouse-move dispatch so widgets can
/// opt-in to a custom cursor without needing to opt-out.
pub fn reset_cursor_icon() {
    CURSOR_ICON.with(|c| c.set(CursorIcon::Default));
}

thread_local! {
    static CURSOR_INVALIDATED: Cell<bool> = const { Cell::new(false) };
}

/// Ask for the hover cursor to be re-resolved without a mouse move.
///
/// For a widget whose cursor changes while the pointer rests on it (a
/// picking mode starting from a key press, a drop target refusing the
/// dragged item): change the state its `MouseMove` handler reads, then
/// call this. [`crate::App::refresh_cursor`] (run at the start of every
/// paint) re-dispatches a move at the last pointer position to the
/// captured widget, or else the hovered one, so only a widget the mouse
/// is over (or that holds the capture) can claim the cursor (agg-sharp's
/// `HoverCursorTests.CursorSetByCodeWhileTheMouseRestsOverTheWidgetReachesTheWindow`).
/// Also requests a frame so the shell wakes to apply it. Call it when the
/// state changes, not from every `MouseMove`, or every frame re-resolves.
pub fn invalidate_cursor() {
    CURSOR_INVALIDATED.with(|c| c.set(true));
    crate::animation::request_draw();
}

/// Take (and clear) a pending [`invalidate_cursor`] request.
pub(crate) fn take_cursor_invalidation() -> bool {
    CURSOR_INVALIDATED.with(|c| c.replace(false))
}

/// What the platform window is showing, so a shell calls the platform only
/// when the cursor actually changes.
///
/// Shells feed [`current_cursor_icon`] in after every input event and every
/// paint; [`WindowCursor::update`] returns the icon only when it differs
/// from the one last applied (agg-sharp's `HoverCursorTests`: "moving
/// without a cursor change must not call the platform again").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowCursor {
    applied: Option<CursorIcon>,
}

impl WindowCursor {
    /// A window that has not had a cursor applied yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// `Some(icon)` when `icon` must be pushed to the platform (it differs
    /// from the last applied one, or nothing has been applied yet).
    pub fn update(&mut self, icon: CursorIcon) -> Option<CursorIcon> {
        if self.applied == Some(icon) {
            None
        } else {
            self.applied = Some(icon);
            Some(icon)
        }
    }

    /// The cursor last applied to the window, if any.
    pub fn applied(&self) -> Option<CursorIcon> {
        self.applied
    }
}
