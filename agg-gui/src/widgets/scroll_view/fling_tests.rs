//! Unit tests for the momentum-scrolling maths in `fling.rs`: the finger's
//! velocity at the lift and the throw's decay, clamping and stop. The
//! behaviour through `App`'s touch entry points is in
//! `crate::tests::scroll_view_fling`.

use std::time::Duration;

use web_time::Instant;

use super::*;

const MS: Duration = Duration::from_millis(1);

/// A finger moving steadily at `speed` (Y-up, px/s) with a sample every
/// `every`, for `samples` samples starting at `t0`.
fn steady(t0: Instant, speed: (f64, f64), every: Duration, samples: u32) -> VelocityTracker {
    let mut tracker = VelocityTracker::new();
    for i in 0..samples {
        let t = every.as_secs_f64() * i as f64;
        tracker.add(
            t0 + every * i,
            Point::new(10.0 + speed.0 * t, 20.0 + speed.1 * t),
        );
    }
    tracker
}

fn near(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

#[test]
fn a_steady_drag_reads_its_own_speed() {
    let t0 = Instant::now();
    let tracker = steady(t0, (-300.0, 1200.0), 16 * MS, 12);
    // lifted one frame after the last move
    let lift = t0 + 16 * MS * 11 + 8 * MS;
    let (vx, vy) = tracker.velocity_at(lift);
    assert!(near(vx, -300.0, 1e-6), "vx {vx}");
    assert!(near(vy, 1200.0, 1e-6), "vy {vy}");
}

#[test]
fn only_the_last_window_counts() {
    // slow for a long while, then a fast flick in the last 100 ms
    let t0 = Instant::now();
    let mut tracker = VelocityTracker::new();
    let mut y = 0.0;
    let mut t = t0;
    for _ in 0..20 {
        tracker.add(t, Point::new(0.0, y));
        y += 1.0;
        t += 16 * MS;
    }
    for _ in 0..8 {
        tracker.add(t, Point::new(0.0, y));
        y += 32.0; // 2000 px/s
        t += 16 * MS;
    }
    // the window reaches back 98 ms from the lift: the last 7 fast samples
    let lift = t - 16 * MS + 2 * MS;
    let (_, vy) = tracker.velocity_at(lift);
    assert!(near(vy, 2000.0, 1e-6), "vy {vy}");
}

#[test]
fn a_finger_that_stops_before_lifting_reads_no_speed() {
    let t0 = Instant::now();
    let tracker = steady(t0, (0.0, 1500.0), 16 * MS, 10);
    let last = t0 + 16 * MS * 9;
    // no events during the pause
    assert_eq!(
        tracker.velocity_at(last + PAUSE_BEFORE_LIFT + MS),
        (0.0, 0.0)
    );
    // within the pause it still flings
    assert!(tracker.velocity_at(last + PAUSE_BEFORE_LIFT - MS).1 > 1000.0);
}

#[test]
fn a_resting_finger_that_keeps_reporting_reads_no_speed() {
    // some platforms keep sending moves of a still finger, which jitters by
    // a fraction of a pixel
    let t0 = Instant::now();
    let mut tracker = steady(t0, (0.0, 1500.0), 16 * MS, 10);
    let rest = Point::new(10.0, 20.0 + 1500.0 * 0.016 * 9.0);
    let mut t = t0 + 16 * MS * 9;
    for i in 0..4 {
        t += 16 * MS;
        let jitter = if i % 2 == 0 { 0.5 } else { -0.5 };
        tracker.add(t, Point::new(rest.x + jitter, rest.y - jitter));
    }
    assert_eq!(tracker.velocity_at(t + 4 * MS), (0.0, 0.0));
}

#[test]
fn too_little_motion_reads_no_speed() {
    let t0 = Instant::now();
    let mut tracker = VelocityTracker::new();
    assert_eq!(tracker.velocity_at(t0), (0.0, 0.0));
    tracker.add(t0, Point::new(0.0, 0.0));
    assert_eq!(tracker.velocity_at(t0 + MS), (0.0, 0.0));
    // two moves at once: one event replayed, not a speed
    tracker.add(t0, Point::new(0.0, 80.0));
    assert_eq!(tracker.velocity_at(t0 + MS), (0.0, 0.0));
    tracker.clear();
    assert_eq!(tracker.velocity_at(t0 + MS), (0.0, 0.0));
}

fn rect() -> Rect {
    Rect::new(0.0, 0.0, 100.0, 100.0)
}

#[test]
fn a_slow_lift_does_not_fling() {
    let t0 = Instant::now();
    assert!(Fling::new(t0, (Some(0.0), None), (MIN_FLING_SPEED - 1.0, 0.0), rect()).is_none());
    assert!(Fling::new(t0, (Some(0.0), None), (MIN_FLING_SPEED + 1.0, 0.0), rect()).is_some());
    // speed along an axis that cannot scroll does not count
    assert!(Fling::new(t0, (Some(0.0), None), (0.0, 5000.0), rect()).is_none());
}

#[test]
fn a_throw_slows_exponentially_and_settles_at_speed_times_the_time_constant() {
    let t0 = Instant::now();
    let speed = 1000.0;
    let mut fling = Fling::new(t0, (Some(100.0), None), (speed, 0.0), rect()).expect("a fling");
    let far = (1.0e6, 0.0);

    let mut last = 100.0;
    let mut last_gain = f64::INFINITY;
    let mut frames = 0;
    let mut t = t0;
    loop {
        t += 16 * MS;
        frames += 1;
        let step = fling.step(t, far);
        let v = step.v.expect("the axis moves until it is done");
        let gain = v - last;
        assert!(gain >= 0.0, "it moves the way it was thrown");
        assert!(
            gain <= last_gain + 1e-9,
            "every frame moves less than the one before"
        );
        last_gain = gain;
        last = v;
        if step.done {
            break;
        }
        assert!(frames < 1000, "the throw ends");
    }
    // one time constant in, the speed is down to 1/e
    let (pos, left) = AxisThrow {
        origin: 100.0,
        speed,
    }
    .at(FLING_TIME_CONSTANT);
    assert!(near(
        left,
        speed * FLING_TIME_CONSTANT / std::f64::consts::E,
        1e-9
    ));
    assert!(pos > 100.0);
    // it lands within the stop distance of where the decay settles: speed x
    // time constant past the start
    let settles = 100.0 + speed * FLING_TIME_CONSTANT;
    assert!(
        last < settles && settles - last < FLING_STOP_DISTANCE,
        "{last}"
    );
    // and stops within a few seconds
    assert!(t - t0 < Duration::from_secs(4), "{:?}", t - t0);
}

#[test]
fn where_a_throw_is_depends_only_on_the_time_since_the_lift() {
    let t0 = Instant::now();
    let mut every_frame = Fling::new(t0, (Some(0.0), None), (2000.0, 0.0), rect()).expect("fling");
    let mut once = Fling::new(t0, (Some(0.0), None), (2000.0, 0.0), rect()).expect("fling");
    let mut v = 0.0;
    for i in 1..=30 {
        v = every_frame
            .step(t0 + 10 * MS * i, (1.0e6, 0.0))
            .v
            .expect("moving");
    }
    let jumped = once.step(t0 + 300 * MS, (1.0e6, 0.0)).v.expect("moving");
    assert_eq!(v, jumped);
}

#[test]
fn a_throw_stops_at_the_contents_ends() {
    let t0 = Instant::now();
    // towards the end: stops exactly at the max
    let mut fling = Fling::new(t0, (Some(380.0), None), (3000.0, 0.0), rect()).expect("fling");
    let step = fling.step(t0 + 200 * MS, (400.0, 0.0));
    assert_eq!(step.v, Some(400.0));
    assert!(step.done);
    // towards the start: stops at 0
    let mut fling = Fling::new(t0, (Some(30.0), None), (-3000.0, 0.0), rect()).expect("fling");
    let step = fling.step(t0 + 200 * MS, (400.0, 0.0));
    assert_eq!(step.v, Some(0.0));
    assert!(step.done);
}

#[test]
fn each_axis_stops_at_its_own_end() {
    let t0 = Instant::now();
    let mut fling =
        Fling::new(t0, (Some(0.0), Some(95.0)), (1000.0, 1000.0), rect()).expect("fling");
    let step = fling.step(t0 + 50 * MS, (1.0e6, 100.0));
    assert_eq!(step.h, Some(100.0));
    assert!(!step.done);
    let step = fling.step(t0 + 100 * MS, (1.0e6, 100.0));
    assert_eq!(step.h, None, "the horizontal axis is done");
    assert!(step.v.is_some());
}

#[test]
fn a_wild_flick_is_capped() {
    let t0 = Instant::now();
    let mut fling = Fling::new(t0, (Some(0.0), None), (1.0e6, 0.0), rect()).expect("fling");
    let mut last = 0.0;
    let mut t = t0;
    loop {
        t += 16 * MS;
        let step = fling.step(t, (1.0e9, 0.0));
        last = step.v.unwrap_or(last);
        if step.done {
            break;
        }
    }
    let settles = MAX_FLING_SPEED * FLING_TIME_CONSTANT;
    assert!(settles - last < FLING_STOP_DISTANCE, "{last}");
}

#[test]
fn a_running_throw_is_registered_until_it_ends() {
    let t0 = Instant::now();
    assert!(!crate::touch_emulation::fling::any_flinging());
    let fling = Fling::new(t0, (Some(0.0), None), (1000.0, 0.0), rect()).expect("fling");
    assert!(crate::touch_emulation::fling::any_flinging());
    assert!(!fling.caught());
    // a finger outside the view leaves it alone
    assert!(!crate::touch_emulation::fling::catch_at(Point::new(
        150.0, 50.0
    )));
    assert!(!fling.caught());
    assert!(crate::touch_emulation::fling::catch_at(Point::new(
        50.0, 50.0
    )));
    assert!(fling.caught());
    drop(fling);
    assert!(!crate::touch_emulation::fling::any_flinging());
}
