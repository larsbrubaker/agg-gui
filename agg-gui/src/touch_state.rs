//! Multi-touch gesture recogniser.
//!
//! The platform shells (web JS, native winit) forward raw touch events
//! to [`App::on_touch_start/move/end/cancel`].  [`TouchState`] maintains
//! the set of active touches and, once two or more fingers are down,
//! aggregates them each frame into a [`MultiTouchInfo`] describing zoom,
//! rotation, pan, and average pressure relative to the previous frame.
//!
//! Widgets that want to react to gestures read the current frame's
//! aggregate via [`current_multi_touch`], a thread-local written by
//! [`App::publish_multi_touch`] at the start of each paint.  Single-
//! finger touches continue to flow through the regular mouse-emulation
//! path, so existing widgets keep working with no changes.
//!
//! The API shape deliberately mirrors egui's (`zoom_delta`,
//! `rotation_delta`, `translation_delta`, `num_touches`, `center_pos`)
//! so ports from egui code read cleanly.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::geometry::Point;

// ---------------------------------------------------------------------------
// Identifier newtypes
// ---------------------------------------------------------------------------

/// Stable per-device identifier.  Different physical input surfaces
/// (e.g. a laptop's built-in touchscreen and a connected tablet) hash
/// to different values.  The web shell always uses `0` (the browser
/// doesn't expose multiple touch devices to pages); winit passes
/// through its device id.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TouchDeviceId(pub u64);

/// Per-finger identifier, stable from Start through End/Cancel.  Re-
/// used after lift — browsers and winit both guarantee identifiers
/// are unique only for the lifetime of the touch.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TouchId(pub u64);

/// Which phase of the gesture this touch event represents.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TouchPhase {
    /// Finger first made contact.
    Start,
    /// Finger moved while in contact.
    Move,
    /// Finger lifted normally.
    End,
    /// Touch was cancelled by the platform (phone call, gesture
    /// hand-off to browser, etc.).
    Cancel,
}

// ---------------------------------------------------------------------------
// MultiTouchInfo — the per-frame aggregate
// ---------------------------------------------------------------------------

/// Gesture aggregate for the current frame, produced when two or more
/// fingers are on the same device.  All deltas are relative to the
/// previous frame's positions — the widget just accumulates them into
/// its own angle / scale / translation state (see `LionView` for the
/// canonical consumer).
#[derive(Copy, Clone, Debug)]
pub struct MultiTouchInfo {
    /// Device that owns these touches.  Useful only when the host
    /// actually distinguishes multiple touchscreens; most apps ignore.
    pub device_id: TouchDeviceId,
    /// Number of fingers currently down (always ≥ 2 — a single-finger
    /// frame produces `None` instead of a [`MultiTouchInfo`]).
    pub num_touches: usize,
    /// Multiplicative zoom factor since the last frame.  `1.0` means
    /// "no pinch this frame"; `1.1` means the fingers spread by 10 %.
    pub zoom_delta: f32,
    /// Rotation in radians since the last frame.  Positive = CCW in
    /// widget-local (Y-up) space, i.e. visually counter-clockwise on
    /// screen.
    pub rotation_delta: f32,
    /// Translation of the centroid since the last frame, in widget-
    /// local pixels.  Widgets that want the gesture to orbit the pinch
    /// centre should combine this with `zoom_delta` / `rotation_delta`.
    pub translation_delta: Point,
    /// Average `force` across active touches, or `0.0` when the
    /// platform doesn't report pressure.
    pub force: f32,
    /// Centroid of the active touches in app-local coordinates this
    /// frame.  Widgets that want to hit-test "is the gesture over me?"
    /// compare this against their own absolute bounds.
    pub center_pos: Point,
}

// ---------------------------------------------------------------------------
// TouchState — per-frame gesture recogniser
// ---------------------------------------------------------------------------

/// One finger's tracked position, updated every Move event.
#[derive(Copy, Clone, Debug)]
struct ActiveTouch {
    /// Latest position reported by the platform.
    pos: Point,
    /// Position at the last `update_gesture` call — used as the basis
    /// for the next delta.
    prev_pos: Point,
    /// Latest force (0.0 when unsupported).
    force: f32,
}

/// Tracks every active touch across every known device.  Lives on
/// `App`; widgets never see this directly.
#[derive(Default)]
pub struct TouchState {
    active: BTreeMap<(TouchDeviceId, TouchId), ActiveTouch>,
    /// Result of the most recent `update_gesture` call — `None` while
    /// fewer than two fingers are down on any one device.  Published
    /// to the thread-local so widgets can read it during paint.
    last: Option<MultiTouchInfo>,
    /// Set by Start / End / Cancel so `update_gesture` can reseed
    /// `prev_pos` on the frame after a finger count change — without
    /// this, newly-arrived fingers contribute a spurious delta equal
    /// to their full spread on their first move.
    topology_changed: bool,
}

impl TouchState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_start(&mut self, device: TouchDeviceId, id: TouchId, pos: Point, force: Option<f32>) {
        self.active.insert(
            (device, id),
            ActiveTouch {
                pos,
                prev_pos: pos,
                force: force.unwrap_or(0.0),
            },
        );
        self.topology_changed = true;
    }

    pub fn on_move(&mut self, device: TouchDeviceId, id: TouchId, pos: Point, force: Option<f32>) {
        if let Some(t) = self.active.get_mut(&(device, id)) {
            t.pos = pos;
            if let Some(f) = force {
                t.force = f;
            }
        }
    }

    pub fn on_end_or_cancel(&mut self, device: TouchDeviceId, id: TouchId) {
        if self.active.remove(&(device, id)).is_some() {
            self.topology_changed = true;
        }
        if self.active.len() < 2 {
            self.last = None;
        }
    }

    /// Recompute the per-frame aggregate.  Called by `App` right before
    /// the multi-touch value is published, so every `paint` / `on_event`
    /// in the same frame sees consistent deltas.
    pub fn update_gesture(&mut self) {
        // Only the most-populated device contributes — the common case
        // is a single touchscreen, and cross-device gestures aren't a
        // useful abstraction.
        let device = self.active.keys().next().map(|(d, _)| *d);
        let Some(device) = device else {
            self.last = None;
            return;
        };
        let touches: Vec<ActiveTouch> = self
            .active
            .iter()
            .filter(|((d, _), _)| *d == device)
            .map(|(_, t)| *t)
            .collect();
        if touches.len() < 2 {
            self.last = None;
            return;
        }

        // Centroid (previous vs current) drives the translation delta.
        let n = touches.len() as f64;
        let (mut cx, mut cy) = (0.0, 0.0);
        let (mut pcx, mut pcy) = (0.0, 0.0);
        for t in &touches {
            cx += t.pos.x;
            cy += t.pos.y;
            pcx += t.prev_pos.x;
            pcy += t.prev_pos.y;
        }
        cx /= n;
        cy /= n;
        pcx /= n;
        pcy /= n;

        // Average pinch + rotation across pairs.  Using every
        // (touch, centroid) ray means the signal scales sensibly with
        // finger count; egui does the same.
        let mut zoom_sum = 0.0_f32;
        let mut rotation_sum = 0.0_f32;
        let mut force_sum = 0.0_f32;
        let mut zoom_count = 0;
        for t in &touches {
            force_sum += t.force;
            let dx = (t.pos.x - cx) as f32;
            let dy = (t.pos.y - cy) as f32;
            let pdx = (t.prev_pos.x - pcx) as f32;
            let pdy = (t.prev_pos.y - pcy) as f32;
            let r = (dx * dx + dy * dy).sqrt();
            let pr = (pdx * pdx + pdy * pdy).sqrt();
            if pr > 1.0 && r > 1.0 {
                zoom_sum += r / pr;
                rotation_sum += dy.atan2(dx) - pdy.atan2(pdx);
                zoom_count += 1;
            }
        }
        // Skip producing a frame-delta when topology just changed —
        // the jump from "no prev_pos" to "current pos" would otherwise
        // read as a huge one-frame zoom.  We still emit an info entry
        // so widgets can react to finger count; just with zeroed
        // deltas.
        let (zoom_delta, rotation_delta) = if self.topology_changed || zoom_count == 0 {
            (1.0, 0.0)
        } else {
            // Normalise rotation to `[-pi, pi]` so wrap-around at the
            // ±pi seam doesn't flip sign of the delta.
            let mut rot = rotation_sum / zoom_count as f32;
            use std::f32::consts::PI;
            while rot > PI {
                rot -= 2.0 * PI;
            }
            while rot < -PI {
                rot += 2.0 * PI;
            }
            (zoom_sum / zoom_count as f32, rot)
        };

        let translation_delta = if self.topology_changed {
            Point::new(0.0, 0.0)
        } else {
            Point::new(cx - pcx, cy - pcy)
        };

        self.last = Some(MultiTouchInfo {
            device_id: device,
            num_touches: touches.len(),
            zoom_delta,
            rotation_delta,
            translation_delta,
            force: force_sum / n as f32,
            center_pos: Point::new(cx, cy),
        });

        // Latch current positions as the new baseline for the next
        // frame, then clear the topology flag.
        for t in self.active.values_mut() {
            t.prev_pos = t.pos;
        }
        self.topology_changed = false;
    }

    pub fn current(&self) -> Option<MultiTouchInfo> {
        self.last
    }

    /// Total number of fingers currently down (across all devices).
    /// Useful as a lightweight "are we in a gesture?" probe when a
    /// widget doesn't care about the per-delta aggregate.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }
}

// ---------------------------------------------------------------------------
// Thread-local publish / read
// ---------------------------------------------------------------------------

thread_local! {
    static CURRENT: RefCell<Option<MultiTouchInfo>> = RefCell::new(None);
}

/// Publish this frame's multi-touch aggregate.  Called by
/// `App::paint` right before painting begins.
pub fn set_current(info: Option<MultiTouchInfo>) {
    CURRENT.with(|c| *c.borrow_mut() = info);
}

/// Fetch the current frame's multi-touch aggregate.  Returns `None`
/// when fewer than two fingers are down on any device, so a widget
/// writes: `if let Some(mt) = current_multi_touch() { … }`.
pub fn current_multi_touch() -> Option<MultiTouchInfo> {
    CURRENT.with(|c| *c.borrow())
}
