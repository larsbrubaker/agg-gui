//! Trackpad pinch and rotate on [`App`]: a magnify gesture delivered as a
//! marked `MouseWheel`, and both gestures as two virtual fingers on the
//! multi-touch path. Split out of `app.rs` (800-line guardrail).
//!
//! Ports agg-sharp's `PlatformMac/mac/MacTrackpadGestures.cs` `Deliver`. A
//! pinch goes out twice. As a wheel, through
//! [`crate::trackpad_pinch::magnification_to_notches`], marked so
//! [`crate::trackpad_pinch::wheel_from_trackpad_pinch`] answers `true` for its
//! receivers (C# `FromTrackpadPinch`): a wheel is what every zoom consumer
//! already reads. And, with the rotation, as two virtual fingers from
//! [`crate::trackpad_pinch_fingers::TrackpadPinchFingers`], applied to the
//! touch recogniser on [`VIRTUAL_TRACKPAD_DEVICE`] for the widgets that follow
//! fingers ([`crate::Event::MultiTouch`]); those skip the marked wheel so they
//! zoom once, and widgets that zoom by the wheel skip that device. The virtual
//! fingers never drive the touch mouse emulation and don't count as a
//! touchscreen touch (`touch_seen_this_session` stays as it was). The native
//! shell calls these from winit's `PinchGesture` and `RotationGesture`.

use super::super::keyboard_scroll;
use crate::event::Modifiers;
use crate::geometry::Point;
use crate::trackpad_pinch::{magnification_to_notches, with_trackpad_pinch_mark};
use crate::trackpad_pinch_fingers::{TrackpadGesturePhase, VIRTUAL_TRACKPAD_DEVICE};
use crate::widget::App;

impl App {
    /// One trackpad magnify event at `(screen_x, screen_y)` (Y-down, like
    /// every other `on_mouse_*` entry point), from a host that reports no
    /// gesture phase: the marked wheel only. With no phase nothing would ever
    /// lift virtual fingers, so a phased host calls
    /// [`Self::on_trackpad_magnify`] instead. `magnification` is the
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

    /// One trackpad magnify event with its phase (C# `Deliver` for
    /// `NSEventTypeMagnify`): the marked wheel of [`Self::on_trackpad_pinch`],
    /// then the virtual fingers' frames. A non-finite magnification is no
    /// change.
    pub fn on_trackpad_magnify(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        magnification: f64,
        phase: TrackpadGesturePhase,
        modifiers: Modifiers,
    ) {
        let magnification = if magnification.is_finite() {
            magnification
        } else {
            0.0
        };
        let pointer = self.trackpad_pointer(screen_x, screen_y);
        let frames = self.trackpad_fingers.magnify(pointer, magnification, phase);
        self.on_trackpad_pinch(screen_x, screen_y, magnification, modifiers);
        self.apply_trackpad_frames(frames);
    }

    /// One trackpad rotate event (C# `Deliver` for `NSEventTypeRotate`):
    /// `degrees` counter-clockwise positive, as AppKit and winit's
    /// `RotationGesture` report it. Only the virtual fingers; a rotation has
    /// no wheel. A non-finite angle is no turn.
    pub fn on_trackpad_rotate(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        degrees: f64,
        phase: TrackpadGesturePhase,
    ) {
        let degrees = if degrees.is_finite() { degrees } else { 0.0 };
        let pointer = self.trackpad_pointer(screen_x, screen_y);
        let frames = self.trackpad_fingers.rotate(pointer, degrees, phase);
        self.apply_trackpad_frames(frames);
    }

    /// The pointer in the app-local (Y-up) space touches are tracked in, by
    /// the same conversion `on_touch_*` uses.
    fn trackpad_pointer(&self, screen_x: f64, screen_y: f64) -> Point {
        keyboard_scroll::lift_to_world(self.flip_y(screen_x, screen_y))
    }

    /// Feeds the virtual fingers' frames to the touch recogniser, oldest
    /// first. The aggregate is formed once per paint, so a gesture's first
    /// event (its rest frame then its change) reaches widgets as one frame
    /// carrying the change.
    fn apply_trackpad_frames(&mut self, frames: Vec<Vec<Point>>) {
        for frame in &frames {
            self.touch_state
                .apply_virtual_frame(VIRTUAL_TRACKPAD_DEVICE, frame);
        }
        // Not published to `touch_points`: nothing is touching the screen.
        crate::animation::request_draw();
    }
}
