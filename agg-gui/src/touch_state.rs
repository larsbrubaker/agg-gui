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
//! `App::paint` at the start of each frame.  Single-finger touches are
//! replayed through the regular mouse pipeline by the core-owned
//! [`crate::touch_emulation::TouchMouseEmu`], so existing widgets keep
//! working with no changes.
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

/// Fold an angle (radians) into the canonical `[-pi, pi]` range.  Used to
/// keep each finger's per-frame rotation step honest across the atan2 ±pi
/// seam before the steps are averaged.
fn wrap_angle(a: f32) -> f32 {
    use std::f32::consts::PI;
    let mut a = a;
    while a > PI {
        a -= 2.0 * PI;
    }
    while a < -PI {
        a += 2.0 * PI;
    }
    a
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

    /// Every active finger's current position, for the per-finger
    /// registry ([`crate::touch_points`]) that virtual-gamepad widgets
    /// poll. Positions are app-local (the space touch events arrive
    /// in after the shell's screen→world conversion).
    pub fn active_points(&self) -> Vec<crate::touch_points::TouchPoint> {
        self.active
            .iter()
            .map(|((_, id), t)| crate::touch_points::TouchPoint {
                id: id.0,
                pos: t.pos,
            })
            .collect()
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
                // Normalise EACH finger's angular step into `[-pi, pi]`
                // before summing.  A real two-finger twist sweeps every
                // finger's polar angle around the centroid, so on every
                // half-turn one finger crosses the atan2 ±pi seam: its raw
                // `atan2(dy,dx) - atan2(pdy,pdx)` jumps by ~±2pi (a true
                // +2° step reads as -358°).  Averaging that with the other
                // finger's correct step yields ~-178°, and post-average
                // normalisation cannot recover the intended small delta.
                // Wrapping per finger keeps each contribution honest.
                rotation_sum += wrap_angle(dy.atan2(dx) - pdy.atan2(pdx));
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
            // Per-finger deltas are already wrapped, so their average is
            // well-behaved; this final wrap is a cheap belt-and-braces
            // clamp for the pathological many-finger case.
            let rot = wrap_angle(rotation_sum / zoom_count as f32);
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
    /// Wall-clock time of the most recent touch lifecycle event
    /// (`Start` / `Move` / `End` / `Cancel`).  Set by `App`'s touch
    /// entry points.  Mouse events the touch shell synthesises arrive
    /// within milliseconds of a touch event — widgets that need to
    /// distinguish a touch tap from a desktop click read
    /// [`last_touch_event_age`] and treat anything under a few tens
    /// of milliseconds as touch-synthesised.
    static LAST_TOUCH_EVENT_AT: std::cell::Cell<Option<web_time::Instant>> =
        const { std::cell::Cell::new(None) };
    /// Sticky "a real touch has happened this session" latch.  Unlike
    /// [`LAST_TOUCH_EVENT_AT`] (which ages out and only distinguishes
    /// touch-synthesised mouse events from desktop clicks), this NEVER
    /// clears on its own: once any touch lifecycle event fires, the
    /// process is treated as touch-driven for UI *sizing* policy (see
    /// [`crate::input_profile::touch_ui_active`]).  Latching, rather than
    /// aging out, means a menu that grew to a finger-friendly size the
    /// instant the user first touched the screen doesn't shrink back to
    /// desktop dimensions a few frames later.  Cleared only by the test
    /// hook [`clear_last_touch_event_for_testing`].
    static TOUCH_SEEN_THIS_SESSION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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

/// Record that a touch lifecycle event just fired.  Called from
/// `App::on_touch_start/move/end/cancel`.
pub(crate) fn note_touch_event() {
    LAST_TOUCH_EVENT_AT.with(|c| c.set(Some(web_time::Instant::now())));
    TOUCH_SEEN_THIS_SESSION.with(|c| c.set(true));
}

/// Whether any real touch lifecycle event has fired this session.  Sticky
/// once set (it does not age out), so it can serve as the runtime-fallback
/// half of the menu sizing-policy signal in
/// [`crate::input_profile::touch_ui_active`]: a phone whose shell forgot to
/// call `set_input_profile` still grows its menus to finger size the moment
/// the user first touches the screen.
pub fn touch_seen_this_session() -> bool {
    TOUCH_SEEN_THIS_SESSION.with(|c| c.get())
}

/// Time elapsed since the most recent touch lifecycle event, or
/// `None` if no touch event has ever fired.  Mouse events
/// synthesised from a touchstart / touchend by the web shell arrive
/// within a millisecond of the touch event — widgets needing to
/// tell touch-synthesised mouse events apart from real desktop
/// clicks check this against a small threshold.
pub fn last_touch_event_age() -> Option<std::time::Duration> {
    LAST_TOUCH_EVENT_AT.with(|c| c.get()).map(|t| t.elapsed())
}

/// Forget any prior touch event so the next mouse event reads as
/// "from desktop" until [`note_touch_event`] runs again.  Tests use
/// this to isolate desktop-mouse scenarios from a sibling test that
/// just simulated a touch tap.
#[doc(hidden)]
pub fn clear_last_touch_event_for_testing() {
    LAST_TOUCH_EVENT_AT.with(|c| c.set(None));
    TOUCH_SEEN_THIS_SESSION.with(|c| c.set(false));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const DEV: TouchDeviceId = TouchDeviceId(0);

    /// Two fingers land, then `update_gesture` runs once.  This is the
    /// baseline (topology-changed) frame: it should emit a `MultiTouchInfo`
    /// with num_touches = 2 but zeroed deltas so newly-arrived fingers
    /// never contribute a spurious one-frame jump.
    #[test]
    fn two_fingers_land_emits_zeroed_baseline_frame() {
        let mut ts = TouchState::new();
        ts.on_start(DEV, TouchId(0), Point::new(100.0, 100.0), None);
        ts.on_start(DEV, TouchId(1), Point::new(200.0, 100.0), None);
        ts.update_gesture();

        let info = ts.current().expect("two fingers should produce a gesture");
        assert_eq!(info.num_touches, 2);
        assert_eq!(info.zoom_delta, 1.0, "topology frame must not zoom");
        assert_eq!(info.rotation_delta, 0.0, "topology frame must not rotate");
        assert_eq!(info.translation_delta.x, 0.0);
        assert_eq!(info.translation_delta.y, 0.0);
    }

    /// Pinch-out: after the baseline frame, both fingers move apart
    /// symmetrically (spread doubles), so `zoom_delta` should equal the
    /// spread ratio and rotation should stay ~0.
    #[test]
    fn pinch_out_reports_spread_ratio() {
        let mut ts = TouchState::new();
        // Baseline: fingers 100px apart, centroid at (150,100).
        ts.on_start(DEV, TouchId(0), Point::new(100.0, 100.0), None);
        ts.on_start(DEV, TouchId(1), Point::new(200.0, 100.0), None);
        ts.update_gesture(); // latches baseline, zeroed deltas

        // Spread to 200px apart around the same centroid.
        ts.on_move(DEV, TouchId(0), Point::new(50.0, 100.0), None);
        ts.on_move(DEV, TouchId(1), Point::new(250.0, 100.0), None);
        ts.update_gesture();

        let info = ts.current().expect("gesture present");
        // Each finger's distance from the centroid went 50 -> 100, so the
        // per-finger ratio (and hence the average) is exactly 2.0.
        assert!(
            (info.zoom_delta - 2.0).abs() < 1e-3,
            "zoom_delta = {} (expected ~2.0)",
            info.zoom_delta
        );
        assert!(
            info.rotation_delta.abs() < 1e-3,
            "pure pinch must not rotate, got {}",
            info.rotation_delta
        );
    }

    /// Pure rotation: both fingers rotate 30° CCW (Y-up) around a fixed
    /// centroid at constant radius.  The code computes
    /// `atan2(dy,dx) - atan2(pdy,pdx)` on the raw coordinates, so a CCW
    /// step in Y-up space yields a POSITIVE `rotation_delta` — matching the
    /// documented "positive = CCW in Y-up" convention.  Zoom must stay ~1.
    #[test]
    fn pure_rotation_reports_signed_angle_ccw_positive() {
        let mut ts = TouchState::new();
        // Fingers on a vertical line through centroid (100,100), r = 100.
        // Vertical orientation keeps both fingers away from the atan2 ±pi
        // seam so no per-finger wrap corrupts the average.
        ts.on_start(DEV, TouchId(0), Point::new(100.0, 200.0), None); // angle +90°
        ts.on_start(DEV, TouchId(1), Point::new(100.0, 0.0), None); //  angle -90°
        ts.update_gesture(); // baseline

        // Rotate both +30° CCW (Y-up) about the centroid.
        //   finger 0: 90° -> 120°  => (50, 186.60254)
        //   finger 1: -90° -> -60° => (150, 13.39746)
        ts.on_move(DEV, TouchId(0), Point::new(50.0, 186.602_54), None);
        ts.on_move(DEV, TouchId(1), Point::new(150.0, 13.397_46), None);
        ts.update_gesture();

        let info = ts.current().expect("gesture present");
        let expected = 30.0_f32.to_radians();
        assert!(
            (info.rotation_delta - expected).abs() < 1e-3,
            "rotation_delta = {} (expected ~+{} rad, +30° CCW)",
            info.rotation_delta,
            expected
        );
        assert!(
            (info.zoom_delta - 1.0).abs() < 1e-3,
            "pure rotation must not zoom, got {}",
            info.zoom_delta
        );
    }

    /// Pure translation: both fingers shift by the same offset, so the
    /// centroid moves by exactly that offset while the per-finger geometry
    /// is unchanged (zoom ~1, rotation ~0).
    #[test]
    fn pure_translation_reports_centroid_offset() {
        let mut ts = TouchState::new();
        ts.on_start(DEV, TouchId(0), Point::new(100.0, 100.0), None);
        ts.on_start(DEV, TouchId(1), Point::new(200.0, 100.0), None);
        ts.update_gesture(); // baseline

        // Shift both by (+10, +20).
        ts.on_move(DEV, TouchId(0), Point::new(110.0, 120.0), None);
        ts.on_move(DEV, TouchId(1), Point::new(210.0, 120.0), None);
        ts.update_gesture();

        let info = ts.current().expect("gesture present");
        assert!(
            (info.translation_delta.x - 10.0).abs() < 1e-3
                && (info.translation_delta.y - 20.0).abs() < 1e-3,
            "translation_delta = ({},{}) (expected ~(10,20))",
            info.translation_delta.x,
            info.translation_delta.y
        );
        assert!(
            (info.zoom_delta - 1.0).abs() < 1e-3,
            "pure translation must not zoom, got {}",
            info.zoom_delta
        );
        assert!(
            info.rotation_delta.abs() < 1e-3,
            "pure translation must not rotate, got {}",
            info.rotation_delta
        );
    }

    /// Lifting one finger drops below the two-finger minimum: `current()`
    /// must become `None` and `active_count` must reflect the remaining
    /// finger.
    #[test]
    fn finger_lift_clears_gesture() {
        let mut ts = TouchState::new();
        ts.on_start(DEV, TouchId(0), Point::new(100.0, 100.0), None);
        ts.on_start(DEV, TouchId(1), Point::new(200.0, 100.0), None);
        ts.update_gesture();
        assert!(ts.current().is_some(), "two fingers -> gesture");
        assert_eq!(ts.active_count(), 2);

        ts.on_end_or_cancel(DEV, TouchId(1));
        assert!(
            ts.current().is_none(),
            "one finger left -> no multi-touch gesture"
        );
        assert_eq!(ts.active_count(), 1);
    }

    /// A third finger arriving mid-gesture flags a topology change, so the
    /// very next `update_gesture` must emit zeroed deltas (no spurious jump)
    /// even though the centroid/geometry shifted as the finger landed.
    #[test]
    fn third_finger_join_resets_deltas() {
        let mut ts = TouchState::new();
        ts.on_start(DEV, TouchId(0), Point::new(100.0, 100.0), None);
        ts.on_start(DEV, TouchId(1), Point::new(200.0, 100.0), None);
        ts.update_gesture(); // baseline

        // Establish a real (non-zero) gesture first.
        ts.on_move(DEV, TouchId(0), Point::new(50.0, 100.0), None);
        ts.on_move(DEV, TouchId(1), Point::new(250.0, 100.0), None);
        ts.update_gesture();
        let mid = ts.current().expect("gesture present");
        assert!(mid.zoom_delta > 1.5, "sanity: mid-gesture was a pinch-out");

        // Third finger lands off-centre; next frame must be zeroed.
        ts.on_start(DEV, TouchId(2), Point::new(150.0, 300.0), None);
        ts.update_gesture();
        let info = ts.current().expect("three fingers -> gesture");
        assert_eq!(info.num_touches, 3);
        assert_eq!(info.zoom_delta, 1.0, "topology reset must zero zoom");
        assert_eq!(
            info.rotation_delta, 0.0,
            "topology reset must zero rotation"
        );
        assert_eq!(info.translation_delta.x, 0.0);
        assert_eq!(info.translation_delta.y, 0.0);
    }

    /// Regression: one finger crosses the atan2 ±pi seam during a small
    /// real two-finger twist.  Both fingers rotate +4° CCW around a fixed
    /// centroid at (200,200), but finger A sits at 178° and steps to 182°
    /// — crossing the seam so its raw per-frame delta reads as ≈ -356°.
    /// Finger B (at -2° -> +2°) reads a clean +4°.  Before the fix the sum
    /// was normalised only AFTER averaging, so the frame delta collapsed to
    /// ≈ -176° instead of +4°.  Per-finger normalisation must repair this.
    #[test]
    fn seam_crossing_finger_does_not_flip_rotation() {
        let mut ts = TouchState::new();
        // Baseline: A at 178°, B at -2°, radius 100 about centroid (200,200).
        ts.on_start(DEV, TouchId(0), Point::new(100.060917, 203.489950), None); // A: 178°
        ts.on_start(DEV, TouchId(1), Point::new(299.939083, 196.510050), None); // B: -2°
        ts.update_gesture(); // baseline (zeroed deltas)

        // Both fingers step +4° CCW: A 178°->182° (crosses +pi seam),
        // B -2°->+2°.
        ts.on_move(DEV, TouchId(0), Point::new(100.060917, 196.510050), None); // A: 182°
        ts.on_move(DEV, TouchId(1), Point::new(299.939083, 203.489950), None); // B: +2°
        ts.update_gesture();

        let info = ts.current().expect("gesture present");
        let expected = 4.0_f32.to_radians();
        assert!(
            (info.rotation_delta - expected).abs() < 1e-3,
            "rotation_delta = {} (expected ~+{} rad, +4° CCW); a seam-crossing \
             finger must not flip the averaged rotation",
            info.rotation_delta,
            expected
        );
        assert!(
            (info.zoom_delta - 1.0).abs() < 1e-3,
            "pure rotation must not zoom, got {}",
            info.zoom_delta
        );
    }

    /// Two `update_gesture` calls with no movement in between: the second
    /// frame (topology already cleared) must read as no-op deltas because
    /// each finger's current position equals its latched baseline.
    #[test]
    fn no_movement_yields_identity_deltas() {
        let mut ts = TouchState::new();
        ts.on_start(DEV, TouchId(0), Point::new(100.0, 100.0), None);
        ts.on_start(DEV, TouchId(1), Point::new(200.0, 100.0), None);
        ts.update_gesture(); // baseline (topology_changed frame)
        ts.update_gesture(); // steady state, no on_move between

        let info = ts.current().expect("gesture present");
        assert!(
            (info.zoom_delta - 1.0).abs() < 1e-6,
            "no movement -> zoom 1.0, got {}",
            info.zoom_delta
        );
        assert!(
            info.rotation_delta.abs() < 1e-6,
            "no movement -> rotation 0, got {}",
            info.rotation_delta
        );
        assert_eq!(info.translation_delta.x, 0.0);
        assert_eq!(info.translation_delta.y, 0.0);
    }
}
