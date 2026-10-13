//! Whether the window is maximized, asked without disturbing it.
//!
//! `shell_loop.rs` asks on every `Resized` and `Moved` (the windowed size and
//! position trackers must not record a maximized rect), and `run.rs` once at
//! start-up. On macOS winit cannot ask a borderless or fixed-size window
//! directly: `isZoomed` only works with a title bar and resize handles, so
//! winit adds them, asks, and takes them away again. Each of those restyles
//! moves the content rect, which AppKit reports as a fresh resize, which the
//! shell answers by asking again — a loop that keeps the main thread busy
//! forever, starving every other event (an app's borderless overlay window
//! never got its input, and its menu-bar menu and global hotkey went dead).
//!
//! A macOS window without both a title bar and resize handles cannot be
//! zoomed by the user anyway, so it reports "not maximized" without asking.
//! Other platforms answer from cached state and are always asked.

use winit::window::Window;

/// Whether `window` is maximized. See the module docs.
pub(crate) fn is_maximized(window: &Window) -> bool {
    maximized_query(window.is_decorated(), window.is_resizable(), || {
        window.is_maximized()
    })
}

/// [`is_maximized`] with the window's style explicit; `ask` is the platform
/// query, called only when asking cannot restyle the window.
fn maximized_query(decorated: bool, resizable: bool, ask: impl FnOnce() -> bool) -> bool {
    if cfg!(target_os = "macos") && !(decorated && resizable) {
        return false;
    }
    ask()
}

#[cfg(test)]
#[path = "maximized_tests.rs"]
mod tests;
