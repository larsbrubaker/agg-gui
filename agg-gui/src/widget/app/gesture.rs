//! Multi-touch gesture routing on [`App`] — the capture machinery that turns
//! the per-frame gesture aggregate into a routed, captured
//! [`Event::MultiTouch`]. Split out of `app.rs` (800-line guardrail).
//!
//! The aggregate itself is recomputed and published to the thread-local by
//! `App::paint` (display-only consumers still read
//! [`crate::current_multi_touch`]); this module is the *event* half. It runs
//! at the top of `paint`, before the paint traversal, so a widget that
//! consumes the event this frame marks its cached window subtree dirty
//! through the ordinary `Consumed → request_draw → epoch-bump → mark_dirty`
//! chain (see [`crate::widget::tree::dispatch_event`]) and re-rasters in the
//! same frame — no gesture-specific dirty hack needed.
//!
//! # Capture semantics
//!
//! On the frame the aggregate transitions `None → Some`, the centroid is
//! hit-tested down the tree exactly like a `MouseDown`. The deepest widget
//! that consumes the event captures the gesture: every later frame delivers
//! along that captured path, even if the centroid drifts outside the widget.
//! If the start hit-test finds no consumer, the gesture routes nowhere for
//! its whole lifetime. The capture clears when the aggregate returns to
//! `None`.

use crate::event::Event;
use crate::widget::tree::dispatch_event;
use crate::widget::App;

impl App {
    /// Route this frame's multi-touch aggregate as a captured
    /// [`Event::MultiTouch`]. Called once per `paint`, right after the
    /// aggregate is recomputed and published.
    pub(super) fn dispatch_gesture(&mut self) {
        match self.touch_state.current() {
            Some(info) => {
                let event = Event::MultiTouch { info };
                if !self.gesture_in_progress {
                    // None → Some: the gesture just started. Hit-test the
                    // centroid the same way a MouseDown routes, deliver once,
                    // and capture the hit path if anything along it consumed.
                    self.gesture_in_progress = true;
                    if let Some(path) = self.compute_hit(info.center_pos) {
                        let consumed =
                            dispatch_event(&mut self.root, &path, &event, info.center_pos)
                                .is_consumed();
                        if consumed {
                            self.gesture_captured = Some(path);
                        }
                    }
                } else if let Some(path) = self.gesture_captured.clone() {
                    // Ongoing gesture: deliver along the captured path every
                    // frame, regardless of where the centroid has drifted.
                    dispatch_event(&mut self.root, &path, &event, info.center_pos);
                }
            }
            None => {
                // Gesture ended (or none this frame): drop the capture so the
                // next gesture re-runs the start hit-test.
                self.gesture_in_progress = false;
                self.gesture_captured = None;
            }
        }
    }
}
