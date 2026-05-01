//! OS-window state persistence helpers.
//!
//! Apps using agg-gui typically want to restore the OS window's size,
//! fullscreen, and maximized state across launches.  This module packages
//! the small serializable record and shared cells that the platform harness
//! writes into / reads from.  The app-specific bits (which demo windows were
//! open, positions of floating `Window` widgets, etc.) are NOT here — those
//! belong to the app's own state struct; this handles only the OS-window
//! level.
//!
//! # Usage sketch
//!
//! ```ignore
//! // Somewhere in app setup:
//! let os_window = agg_gui::OsWindowHandle::new();
//!
//! // In winit's Resized handler:
//! os_window.size.set((new_size.width, new_size.height));
//! os_window.fullscreen.set(window.fullscreen().is_some());
//! os_window.maximized.set(window.is_maximized());
//!
//! // On close / every idle tick, serialize `OsWindowState::from_handle(&os_window)`
//! // and write it wherever app state lives.  On startup, parse it back and
//! // apply via `WindowAttributes::with_inner_size` / `with_maximized` /
//! // `with_fullscreen`, then after the window is live call
//! // `window.set_fullscreen(...)` / `set_maximized(true)` as a
//! // belt-and-suspenders (some platforms ignore the initial attribute).
//! ```
//!
//! See `demo-native/src/main.rs` for a complete wiring.

use std::cell::Cell;
use std::rc::Rc;

/// Snapshot of the OS window state — serialisable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct OsWindowState {
    /// Logical width in pixels.  `None` = no saved size (use app default).
    pub width: Option<u32>,
    /// Logical height in pixels.
    pub height: Option<u32>,
    /// Window was borderless-fullscreen.
    pub fullscreen: bool,
    /// Window was maximized (not fullscreen).
    pub maximized: bool,
}

impl OsWindowState {
    /// Read the shared cells of an [`OsWindowHandle`] into a snapshot.
    pub fn from_handle(h: &OsWindowHandle) -> Self {
        let (w, hgt) = h.size.get();
        Self {
            width: if w > 0 { Some(w) } else { None },
            height: if hgt > 0 { Some(hgt) } else { None },
            fullscreen: h.fullscreen.get(),
            maximized: h.maximized.get(),
        }
    }

    /// Compact one-line form: `width,height,fullscreen,maximized` (integers,
    /// comma-separated).  Missing dimensions write as `0`.
    pub fn serialize(&self) -> String {
        format!(
            "{},{},{},{}",
            self.width.unwrap_or(0),
            self.height.unwrap_or(0),
            self.fullscreen as u8,
            self.maximized as u8,
        )
    }

    /// Parse the format produced by [`OsWindowState::serialize`].  Returns
    /// `None` on malformed input.  Backward-compatible: a 3-field
    /// `width,height,fullscreen` input (no maximized) parses with
    /// `maximized = false`.
    pub fn deserialize(s: &str) -> Option<Self> {
        let mut it = s.splitn(4, ',');
        let w: u32 = it.next()?.trim().parse().ok()?;
        let h: u32 = it.next()?.trim().parse().ok()?;
        let fs: u8 = it.next()?.trim().parse().ok()?;
        let mx: u8 = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(0);
        Some(Self {
            width: if w > 0 { Some(w) } else { None },
            height: if h > 0 { Some(h) } else { None },
            fullscreen: fs != 0,
            maximized: mx != 0,
        })
    }
}

/// Live shared cells the platform harness mutates during the event loop.
///
/// Cheap to clone — all fields are `Rc<Cell<...>>`.  The harness updates
/// `size` on every `Resized`, `fullscreen` / `maximized` on the
/// corresponding state-change events.  Widgets that want to reflect the
/// current OS window state read the cells directly.
#[derive(Clone)]
pub struct OsWindowHandle {
    pub size: Rc<Cell<(u32, u32)>>,
    pub fullscreen: Rc<Cell<bool>>,
    pub maximized: Rc<Cell<bool>>,
}

impl OsWindowHandle {
    pub fn new() -> Self {
        Self {
            size: Rc::new(Cell::new((0, 0))),
            fullscreen: Rc::new(Cell::new(false)),
            maximized: Rc::new(Cell::new(false)),
        }
    }

    /// Load an initial snapshot into the cells so first-frame code sees the
    /// about-to-be-applied state before any `Resized` event fires.
    pub fn apply(&self, s: &OsWindowState) {
        let w = s.width.unwrap_or(0);
        let h = s.height.unwrap_or(0);
        self.size.set((w, h));
        self.fullscreen.set(s.fullscreen);
        self.maximized.set(s.maximized);
    }
}

impl Default for OsWindowHandle {
    fn default() -> Self {
        Self::new()
    }
}
