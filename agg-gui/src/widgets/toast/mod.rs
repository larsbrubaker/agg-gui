//! Toasts: short transient messages ("Copied path", "Saved", an error)
//! stacked in a corner over the app, each gone after a few seconds.
//!
//! Two halves:
//!
//! * [`Toasts`] — a cheap `Clone` handle (`Rc` inside) to the message queue.
//!   Clone it into any closure on the UI thread and call
//!   [`Toasts::show`] / [`Toasts::show_kind`] / [`Toasts::show_toast`].
//! * [`ToastHost`] (`host.rs`) — a widget that lays out one child over its
//!   whole area and paints the queue's toasts on top of it, at a corner
//!   ([`ToastHost::with_anchor`]).  The pointer over a toast pauses every
//!   countdown; a click on one dismisses it; elsewhere the child gets the
//!   pointer as usual.
//!
//! A toast may carry a [`ToastKind`] (info / success / warning / error),
//! shown as a Font Awesome icon and an accent stripe in the kind's theme
//! colour ([`ToastKind::accent`]).  Toasts fade in and out; the timing
//! state machine lives in `queue.rs` and runs on the UI clock
//! (`crate::clock`), so tests drive it with the virtual clock.

mod host;
mod queue;

pub use host::{PaintedToast, ToastHost};
pub use queue::{FADE_IN as TOAST_FADE_IN, FADE_OUT as TOAST_FADE_OUT};

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::color::Color;
use crate::theme::Visuals;

use queue::Queue;

/// How long a toast stays up by default (pauses excluded).
pub const DEFAULT_TOAST_DURATION: Duration = Duration::from_secs(4);
/// How long an [`ToastKind::Error`] toast stays up by default.
pub const ERROR_TOAST_DURATION: Duration = Duration::from_secs(8);

/// What a toast reports; picks its icon and accent colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

impl ToastKind {
    /// The Font Awesome 4.7 icon code point (needs a font with the Font
    /// Awesome face, e.g. [`crate::fonts::standard_ui_font`]).
    pub fn icon(self) -> char {
        match self {
            ToastKind::Info => '\u{F05A}',    // info-circle
            ToastKind::Success => '\u{F058}', // check-circle
            ToastKind::Warning => '\u{F071}', // exclamation-triangle
            ToastKind::Error => '\u{F06A}',   // exclamation-circle
        }
    }

    /// The kind's accent colour in palette `v`: the theme accent for info,
    /// [`Visuals::success_color`] / [`Visuals::warning_color`] /
    /// [`Visuals::error_color`] for the others.
    pub fn accent(self, v: &Visuals) -> Color {
        match self {
            ToastKind::Info => v.accent,
            ToastKind::Success => v.success_color(),
            ToastKind::Warning => v.warning_color(),
            ToastKind::Error => v.error_color(),
        }
    }
}

/// One message to show, built with the `with_*` methods and passed to
/// [`Toasts::show_toast`].
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    text: String,
    kind: Option<ToastKind>,
    /// `None`: the default for the kind.  `Some(None)`: sticky.
    duration: Option<Option<Duration>>,
}

impl Toast {
    /// A plain toast (no kind, no icon) with the default duration.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: None,
            duration: None,
        }
    }

    pub fn with_kind(mut self, kind: ToastKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Stay up for `duration` of unpaused time instead of the default.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(Some(duration));
        self
    }

    /// Stay up until clicked or dismissed in code.
    pub fn sticky(mut self) -> Self {
        self.duration = Some(None);
        self
    }

    /// The countdown it gets: its own, else [`ERROR_TOAST_DURATION`] for
    /// errors and [`DEFAULT_TOAST_DURATION`] otherwise; `None` is sticky.
    fn resolved_duration(&self) -> Option<Duration> {
        self.duration.unwrap_or(Some(match self.kind {
            Some(ToastKind::Error) => ERROR_TOAST_DURATION,
            _ => DEFAULT_TOAST_DURATION,
        }))
    }
}

/// Identifies one shown toast, for [`Toasts::dismiss`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToastId(u64);

/// Shared handle to a toast queue; see the module docs.  UI thread only
/// (`Rc`).  Every clone refers to the same queue.
#[derive(Clone, Default)]
pub struct Toasts {
    queue: Rc<RefCell<Queue>>,
}

impl std::fmt::Debug for Toasts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Toasts")
            .field("texts", &self.texts())
            .finish()
    }
}

impl Toasts {
    pub fn new() -> Self {
        Self::default()
    }

    /// Show at most `n` toasts at once (default 5); older ones fade out
    /// when a new one would exceed it.
    pub fn with_max_visible(self, n: usize) -> Self {
        self.set_max_visible(n);
        self
    }

    pub fn set_max_visible(&self, n: usize) {
        self.queue.borrow_mut().max_visible = n.max(1);
    }

    /// Show a plain `text` toast.
    pub fn show(&self, text: impl Into<String>) -> ToastId {
        self.show_toast(Toast::new(text))
    }

    /// Show `text` as a `kind` toast (icon and accent colour).
    pub fn show_kind(&self, kind: ToastKind, text: impl Into<String>) -> ToastId {
        self.show_toast(Toast::new(text).with_kind(kind))
    }

    /// Show a toast built with [`Toast`]'s options.
    pub fn show_toast(&self, toast: Toast) -> ToastId {
        let id = self.queue.borrow_mut().push(toast, crate::clock::now());
        crate::animation::request_draw();
        id
    }

    /// Fade out toast `id` now (no-op once it is gone or already fading).
    pub fn dismiss(&self, id: ToastId) {
        if self.queue.borrow_mut().dismiss(id, crate::clock::now()) {
            crate::animation::request_draw();
        }
    }

    /// Fade out every toast.
    pub fn dismiss_all(&self) {
        self.queue.borrow_mut().dismiss_all(crate::clock::now());
        crate::animation::request_draw();
    }

    /// Texts of the toasts up now, oldest first — including ones still
    /// fading out.
    pub fn texts(&self) -> Vec<String> {
        let mut q = self.queue.borrow_mut();
        q.tick(crate::clock::now());
        q.entries.iter().map(|e| e.text.clone()).collect()
    }

    /// Number of toasts up now (including ones fading out).
    pub fn len(&self) -> usize {
        let mut q = self.queue.borrow_mut();
        q.tick(crate::clock::now());
        q.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the countdowns are paused (the pointer is over a toast).
    pub fn is_paused(&self) -> bool {
        self.queue.borrow().is_paused()
    }

    fn queue(&self) -> std::cell::RefMut<'_, Queue> {
        self.queue.borrow_mut()
    }
}
