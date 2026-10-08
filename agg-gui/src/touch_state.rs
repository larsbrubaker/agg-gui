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
//! A trackpad's pinch and rotate arrive here too, as two virtual fingers on
//! [`crate::trackpad_pinch_fingers::VIRTUAL_TRACKPAD_DEVICE`]
//! ([`TouchState::apply_virtual_frame`], fed by `App::on_trackpad_magnify` /
//! `on_trackpad_rotate`); they never drive the mouse emulation.
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
    /// `f64`, as agg-sharp's `MultiTouchInfo.ZoomDelta`, so a virtual
    /// trackpad pinch reads back its magnification exactly.
    pub zoom_delta: f64,
    /// Rotation in radians since the last frame.  Positive = CCW in
    /// widget-local (Y-up) space, i.e. visually counter-clockwise on
    /// screen.
    pub rotation_delta: f64,
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
}

/// Fold an angle (radians) into the canonical `[-pi, pi]` range.  Used to
/// keep each finger's per-frame rotation step honest across the atan2 ±pi
/// seam before the steps are averaged.
fn wrap_angle(a: f64) -> f64 {
    use std::f64::consts::PI;
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
        self.latch_baseline();
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
            self.latch_baseline();
        }
        if self.active.len() < 2 {
            self.last = None;
        }
    }

    /// A finger landed or lifted: every finger's current position becomes
    /// the baseline the next [`Self::update_gesture`] measures from.
    ///
    /// Comparing across a count change would read the new finger's whole
    /// spread as a one-frame zoom, so the change itself reports nothing - but
    /// moves made *after* it, before the next frame is aggregated, are real
    /// and are kept. (Zeroing the whole next frame instead would drop a
    /// virtual trackpad pinch's first event, which lands its fingers and
    /// moves them before a single paint.)
    fn latch_baseline(&mut self) {
        for t in self.active.values_mut() {
            t.prev_pos = t.pos;
        }
    }

    /// Applies one frame of positions on `device`, as agg-sharp's
    /// `MultiTouchGesture.Update` takes a multi-position mouse move: finger
    /// `i` is touch id `i`. Fingers past the frame's count lift; a frame of
    /// fewer than two positions ends the whole gesture on that device (every
    /// consumer ends its pinch on a single-position frame). Used for the
    /// virtual trackpad fingers ([`crate::trackpad_pinch_fingers`]), which
    /// never drive the mouse emulation real touches do.
    pub fn apply_virtual_frame(&mut self, device: TouchDeviceId, frame: &[Point]) {
        let keep = if frame.len() < 2 { 0 } else { frame.len() };
        let lifted: Vec<(TouchDeviceId, TouchId)> = self
            .active
            .keys()
            .filter(|(d, id)| *d == device && id.0 as usize >= keep)
            .copied()
            .collect();
        for (d, id) in lifted {
            self.on_end_or_cancel(d, id);
        }
        for (i, pos) in frame.iter().enumerate().take(keep) {
            let id = TouchId(i as u64);
            if self.active.contains_key(&(device, id)) {
                self.on_move(device, id, *pos, None);
            } else {
                self.on_start(device, id, *pos, None);
            }
        }
    }

    /// Every active finger's current position, for the per-finger
    /// registry ([`crate::touch_points`]) that virtual-gamepad widgets
    /// poll. Positions are app-local (the space touch events arrive
    /// in after the shell's screen→world conversion). The virtual trackpad
    /// fingers are not on the screen, so they are left out.
    pub fn active_points(&self) -> Vec<crate::touch_points::TouchPoint> {
        self.active
            .iter()
            .filter(|((d, _), _)| *d != crate::trackpad_pinch_fingers::VIRTUAL_TRACKPAD_DEVICE)
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
        // Ties go to the lowest device id, so a real touchscreen wins over
        // the virtual trackpad fingers.
        let mut counts: BTreeMap<TouchDeviceId, usize> = BTreeMap::new();
        for (d, _) in self.active.keys() {
            *counts.entry(*d).or_default() += 1;
        }
        let device = counts
            .iter()
            .fold(None::<(TouchDeviceId, usize)>, |best, (d, n)| match best {
                Some((_, bn)) if bn >= *n => best,
                _ => Some((*d, *n)),
            })
            .map(|(d, _)| d);
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
        let mut zoom_sum = 0.0_f64;
        let mut rotation_sum = 0.0_f64;
        let mut force_sum = 0.0_f32;
        let mut zoom_count = 0;
        for t in &touches {
            force_sum += t.force;
            let dx = t.pos.x - cx;
            let dy = t.pos.y - cy;
            let pdx = t.prev_pos.x - pcx;
            let pdy = t.prev_pos.y - pcy;
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
        // A finger count change already latched every baseline
        // (`latch_baseline`), so the deltas here only ever cover moves
        // made since the fingers that are down now were all down.
        let (zoom_delta, rotation_delta) = if zoom_count == 0 {
            (1.0, 0.0)
        } else {
            // Per-finger deltas are already wrapped, so their average is
            // well-behaved; this final wrap is a cheap belt-and-braces
            // clamp for the pathological many-finger case.
            let rot = wrap_angle(rotation_sum / zoom_count as f64);
            (zoom_sum / zoom_count as f64, rot)
        };

        let translation_delta = Point::new(cx - pcx, cy - pcy);

        self.last = Some(MultiTouchInfo {
            device_id: device,
            num_touches: touches.len(),
            zoom_delta,
            rotation_delta,
            translation_delta,
            force: force_sum / n as f32,
            center_pos: Point::new(cx, cy),
        });

        // Latch current positions as the new baseline for the next frame.
        self.latch_baseline();
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
    LAST_TOUCH_EVENT_AT.with(|c| c.set(Some(crate::clock::now())));
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
    LAST_TOUCH_EVENT_AT
        .with(|c| c.get())
        .map(crate::clock::since)
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
        let expected = 30.0_f64.to_radians();
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
        let expected = 4.0_f64.to_radians();
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

    /// Fingers that land and move before the next `update_gesture` (a
    /// virtual trackpad pinch's first event, or a fast real pinch between
    /// two paints) keep the move: the landing latched the baseline, so only
    /// the landing itself reads as no change.
    #[test]
    fn moves_after_landing_count_in_the_first_frame() {
        let mut ts = TouchState::new();
        ts.on_start(DEV, TouchId(0), Point::new(100.0, 100.0), None);
        ts.on_start(DEV, TouchId(1), Point::new(200.0, 100.0), None);
        ts.on_move(DEV, TouchId(0), Point::new(50.0, 100.0), None);
        ts.on_move(DEV, TouchId(1), Point::new(250.0, 100.0), None);
        ts.update_gesture();

        let info = ts.current().expect("gesture present");
        assert!(
            (info.zoom_delta - 2.0).abs() < 1e-12,
            "zoom_delta = {} (expected 2.0)",
            info.zoom_delta
        );
        assert_eq!(info.translation_delta.x, 0.0);
        assert_eq!(info.translation_delta.y, 0.0);
    }

    /// The device with the most fingers down drives the aggregate, so one
    /// resting finger on a touchscreen doesn't hide a two-finger gesture on
    /// another device.
    #[test]
    fn the_device_with_most_fingers_drives_the_gesture() {
        let mut ts = TouchState::new();
        let other = TouchDeviceId(7);
        ts.on_start(DEV, TouchId(0), Point::new(10.0, 10.0), None);
        ts.on_start(other, TouchId(0), Point::new(100.0, 100.0), None);
        ts.on_start(other, TouchId(1), Point::new(200.0, 100.0), None);
        ts.update_gesture();

        let info = ts.current().expect("two fingers on one device");
        assert_eq!(info.device_id, other);
        assert_eq!(info.num_touches, 2);
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
