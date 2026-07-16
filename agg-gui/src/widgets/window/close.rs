//! Close-semantics types for [`super::Window`]: the reason a window closed and
//! the policy for pointer presses that land outside a modal window.
//!
//! These live in their own module so the main `window.rs` stays under the
//! 800-line guardrail. [`CloseReason`] is threaded through the unified
//! [`Window::close`](super::Window) path and delivered to the `on_close`
//! callback, letting a dialog distinguish a *commit-style* dismissal
//! (click-away, when a live change should be kept) from a *cancel-style* one
//! (Escape / the × button, which restore prior state).

/// Why a [`Window`](super::Window) is closing. Delivered to the `on_close`
/// callback so a host can react differently per route — the colour dialog
/// commits a live preview on [`ClickAway`](CloseReason::ClickAway) but cancels
/// it on [`Escape`](CloseReason::Escape) / [`CloseButton`](CloseReason::CloseButton).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    /// The user clicked the title-bar × button.
    CloseButton,
    /// The user pressed Escape while the modal window held focus.
    Escape,
    /// The user pressed the pointer outside a click-away-enabled modal window.
    ClickAway,
}

/// What a modal [`Window`](super::Window) does when the pointer is pressed
/// outside its own bounds.
///
/// The modal grab routes *every* press to the window's subtree (so nothing
/// leaks to widgets painted beneath); this decides what the window does with an
/// outside press once it arrives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClickAwayAction {
    /// Swallow the outside press and stay open (the default — preserves the
    /// behaviour of plain modal windows).
    #[default]
    None,
    /// Close via the unified [`Window::close`](super::Window) path with
    /// [`CloseReason::ClickAway`], and swallow the press so it never activates
    /// whatever sat underneath.
    Close,
}
