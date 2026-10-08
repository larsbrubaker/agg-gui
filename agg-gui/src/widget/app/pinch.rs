//! Trackpad pinch on [`App`]: a magnify gesture delivered as a marked
//! `MouseWheel`. Split out of `app.rs` (800-line guardrail).
//!
//! Ports the wheel half of agg-sharp's `PlatformMac/mac/MacTrackpadGestures.cs`
//! `Deliver`: the magnification becomes a wheel delta through
//! [`crate::trackpad_pinch::magnification_to_notches`] and is dispatched as a
//! wheel at the pointer, marked so [`crate::trackpad_pinch::wheel_from_trackpad_pinch`]
//! answers `true` for its receivers (C# `FromTrackpadPinch`). The arithmetic
//! lives in [`crate::trackpad_pinch`]; the native shell calls this from
//! winit's `PinchGesture`.

use crate::event::Modifiers;
use crate::trackpad_pinch::{magnification_to_notches, with_trackpad_pinch_mark};
use crate::widget::App;

impl App {
    /// One trackpad magnify event at `(screen_x, screen_y)` (Y-down, like
    /// every other `on_mouse_*` entry point). `magnification` is the
    /// incremental change of scale for this event, as AppKit's (and winit's
    /// `PinchGesture`) reports it: `0.1` is 10% bigger, positive is fingers
    /// apart, which arrives as a forward wheel (zoom in).
    pub fn on_trackpad_pinch(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        magnification: f64,
        modifiers: Modifiers,
    ) {
        let notches = magnification_to_notches(magnification);
        with_trackpad_pinch_mark(|| {
            self.on_mouse_wheel_xy_mods(screen_x, screen_y, 0.0, notches, modifiers)
        });
    }
}
