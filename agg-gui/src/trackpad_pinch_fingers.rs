//! Virtual trackpad fingers: a trackpad's pinch and rotate, which a mac
//! reports as a scale and an angle (never as finger positions), turned into
//! two virtual fingers either side of the pointer.
//!
//! Ports agg-sharp's `Gui/MultiTouch/TrackpadPinchFingers.cs` (and its
//! `TrackpadGesturePhase`). agg-sharp feeds the frames into
//! `MultiTouchGesture` as two-position mouse moves; agg-gui feeds them into
//! the same [`crate::touch_state::TouchState`] real touches use, on their own
//! device ([`VIRTUAL_TRACKPAD_DEVICE`], see
//! [`crate::touch_state::TouchState::apply_virtual_frame`]), so every widget
//! that follows fingers through [`crate::Event::MultiTouch`] follows a
//! trackpad too. The shells deliver the gestures through
//! `App::on_trackpad_magnify` / `App::on_trackpad_rotate`
//! (`widget/app/pinch.rs`); the wheel half of a pinch is
//! [`crate::trackpad_pinch`].
//!
//! The fingers sit symmetrically about the pointer, so their centre never
//! moves (no pan) while their spread is the running scale and their angle the
//! running rotation: the recogniser then reads back exactly the reported
//! magnification and turn. Positions are app-local (Y-up), so a positive
//! angle is counter-clockwise on screen, as the recogniser reports it.

use crate::geometry::Point;
use crate::touch_state::TouchDeviceId;

/// The touch device the virtual trackpad fingers are reported on, distinct
/// from every real touchscreen (winit device ids and the web shell's `0`).
/// A consumer that already reads the pinch's marked wheel
/// ([`crate::trackpad_pinch::wheel_from_trackpad_pinch`]) skips gestures
/// from this device so it zooms once.
pub const VIRTUAL_TRACKPAD_DEVICE: TouchDeviceId = TouchDeviceId(u64::MAX);

/// Each virtual finger's starting distance from the pointer, in design units
/// (C# `MacTrackpadGestures.FingerRadius`). C# scales it by the device scale
/// because its events are in device pixels; agg-gui's app-local positions are
/// already logical, so the design units are used as they are.
pub const TRACKPAD_FINGER_RADIUS: f64 = 50.0;

/// Where one trackpad gesture event sits in its gesture's life.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TrackpadGesturePhase {
    /// The first event of a pinch or rotation: the fingers are placed about
    /// the pointer afresh.
    Began,
    /// Any event in the middle, and any event from a device that reports no
    /// phase.
    Changed,
    /// The fingers lifted (or the gesture was cancelled).
    Ended,
}

/// Keeps a pinch all the way in from bringing the fingers together, where
/// they would have no angle.
const MINIMUM_SCALE: f64 = 0.05;

/// Turns trackpad pinch and rotate events into two virtual finger positions
/// per frame (C# `TrackpadPinchFingers`).
#[derive(Clone, Debug)]
pub struct TrackpadPinchFingers {
    radius: f64,
    anchor: Point,
    scale: f64,
    angle: f64,
    active: bool,
}

impl TrackpadPinchFingers {
    /// `radius` is how far each finger starts from the pointer, in the
    /// pixels the frames are sent in.
    pub fn new(radius: f64) -> Self {
        Self {
            radius,
            anchor: Point::new(0.0, 0.0),
            scale: 1.0,
            angle: 0.0,
            active: false,
        }
    }

    /// True between a gesture's first event and its end.
    pub fn active(&self) -> bool {
        self.active
    }

    /// Folds in one pinch event. `magnification` is the change of scale this
    /// event, as NSEvent's magnification: 0.1 is 10% bigger. Returns the
    /// frames to send, oldest first: each the positions of one frame.
    pub fn magnify(
        &mut self,
        pointer: Point,
        magnification: f64,
        phase: TrackpadGesturePhase,
    ) -> Vec<Vec<Point>> {
        self.step(pointer, phase, |f| {
            f.scale = MINIMUM_SCALE.max(f.scale * (1.0 + magnification));
        })
    }

    /// Folds in one rotate event. `degrees` is the turn this event, as
    /// NSEvent's rotation: counter-clockwise positive.
    pub fn rotate(
        &mut self,
        pointer: Point,
        degrees: f64,
        phase: TrackpadGesturePhase,
    ) -> Vec<Vec<Point>> {
        self.step(pointer, phase, |f| {
            f.angle += degrees * std::f64::consts::PI / 180.0;
        })
    }

    fn step(
        &mut self,
        pointer: Point,
        phase: TrackpadGesturePhase,
        apply: impl FnOnce(&mut Self),
    ) -> Vec<Vec<Point>> {
        let mut frames = Vec::new();
        if !self.active || phase == TrackpadGesturePhase::Began {
            // A frame at rest first: a recogniser takes the first two-finger
            // frame as its baseline, and this event's own change must not be
            // spent on that.
            self.active = true;
            self.anchor = pointer;
            self.scale = 1.0;
            self.angle = 0.0;
            frames.push(self.fingers());
        }

        apply(self);
        frames.push(self.fingers());

        if phase == TrackpadGesturePhase::Ended {
            // One finger left, where the pointer is: every consumer ends its
            // pinch on a single-position frame.
            self.active = false;
            frames.push(vec![self.anchor]);
        }

        frames
    }

    fn fingers(&self) -> Vec<Point> {
        let r = self.radius * self.scale;
        let (ox, oy) = (self.angle.cos() * r, self.angle.sin() * r);
        vec![
            Point::new(self.anchor.x - ox, self.anchor.y - oy),
            Point::new(self.anchor.x + ox, self.anchor.y + oy),
        ]
    }
}
