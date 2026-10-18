//! # Fullscreen toggle requests — app → platform shell
//!
//! Apps can't touch the winit window or the browser document, so a
//! fullscreen toggle is a request: the app calls [`request_toggle`]
//! (from a click or keydown handler — browsers demand a user gesture),
//! and the platform shell polls [`take_request`] once per frame and
//! performs the switch (winit `set_fullscreen` natively,
//! `requestFullscreen`/`exitFullscreen` on the canvas in a browser).
//! Shells report the resulting state through [`set_active`] so apps
//! can render the right icon via [`is_active`].

use std::sync::atomic::{AtomicBool, Ordering};

static PENDING: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Ask the platform shell to toggle fullscreen (e.g. from an Alt+Enter
/// handler or a UI button). Coalesces: several requests in one frame
/// still toggle once.
pub fn request_toggle() {
    PENDING.store(true, Ordering::Relaxed);
    crate::frame_demand::notify();
}

/// Shell side: consume a pending toggle request, if any.
pub fn take_request() -> bool {
    PENDING.swap(false, Ordering::Relaxed)
}

/// Shell side: whether a toggle is waiting, without consuming it — so a
/// loop that sleeps while idle runs once more to serve it.
pub fn requested() -> bool {
    PENDING.load(Ordering::Relaxed)
}

/// Shell side: record the actual state after switching (or after the
/// user left fullscreen through the OS/browser, e.g. Esc).
pub fn set_active(active: bool) {
    ACTIVE.store(active, Ordering::Relaxed);
}

/// The last state a shell reported — drives expand/compress icons.
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    std::thread_local! {
        static NOTIFIED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    fn count_notify() {
        NOTIFIED.with(|c| c.set(c.get() + 1));
    }

    #[test]
    fn request_is_consumed_once_and_state_tracks() {
        assert!(!take_request());
        assert!(!requested());
        crate::frame_demand::set_hook(Some(count_notify));
        request_toggle();
        request_toggle();
        crate::frame_demand::set_hook(None);
        assert_eq!(
            NOTIFIED.with(|c| c.get()),
            2,
            "each request wakes a sleeping shell loop"
        );
        assert!(requested(), "peeking does not consume");
        assert!(take_request(), "request should be pending");
        assert!(!take_request(), "request must coalesce and clear");
        set_active(true);
        assert!(is_active());
        set_active(false);
        assert!(!is_active());
    }
}
