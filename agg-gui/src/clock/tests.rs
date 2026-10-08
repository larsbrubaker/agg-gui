//! Unit tests for [`crate::clock`]: the virtual clock itself, and the timing
//! state machines that read it (animation deadlines, `Tween`, the text
//! editors' multi-click window). Tooltip delays on the virtual clock are
//! covered in `widgets/tooltip/tests.rs`.

use super::*;
use crate::animation::{
    clear_draw_request, peek_next_draw_deadline, request_draw_after, take_layout_request,
    wants_draw, Tween,
};
use crate::geometry::Point;
use crate::widgets::multi_click::MultiClickTracker;

#[test]
fn virtual_clock_stands_still_until_advanced() {
    let _g = scoped_virtual(None);
    assert!(is_virtual());
    let start = now();
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(now(), start, "virtual time does not follow the wall clock");
    advance(Duration::from_millis(250));
    assert_eq!(now(), start + Duration::from_millis(250));
    assert_eq!(since(start), Duration::from_millis(250));
}

#[test]
fn advancing_the_real_clock_does_nothing() {
    use_real();
    advance(Duration::from_secs(3600));
    assert!(!is_virtual());
    let real = Instant::now();
    assert!(now() >= real && now() < real + Duration::from_secs(60));
}

#[test]
fn scoped_virtual_restores_the_previous_clock() {
    use_real();
    {
        let base = Instant::now();
        let _g = scoped_virtual(Some(base));
        assert_eq!(now(), base);
        {
            let _inner = scoped_virtual(Some(base + Duration::from_secs(1)));
            advance(Duration::from_secs(1));
            assert_eq!(now(), base + Duration::from_secs(2));
        }
        assert_eq!(
            now(),
            base,
            "the inner guard restores the outer virtual time"
        );
    }
    assert!(!is_virtual(), "the outer guard restores the real clock");
}

#[test]
fn since_saturates_for_a_timestamp_in_the_future() {
    let base = Instant::now();
    let _g = scoped_virtual(Some(base));
    assert_eq!(since(base + Duration::from_secs(1)), Duration::ZERO);
}

#[test]
fn draw_deadline_comes_due_when_the_virtual_clock_reaches_it() {
    let _g = scoped_virtual(None);
    clear_draw_request();
    take_layout_request();
    request_draw_after(Duration::from_millis(100));
    let deadline = peek_next_draw_deadline().expect("deadline armed");
    assert_eq!(deadline, now() + Duration::from_millis(100));

    advance(Duration::from_millis(99));
    // `wants_draw` may report a cross-thread async bump from a parallel test,
    // but it must not consume a deadline that has not come due.
    let _ = wants_draw();
    assert_eq!(
        peek_next_draw_deadline(),
        Some(deadline),
        "not yet due at 99 ms"
    );

    advance(Duration::from_millis(1));
    assert!(wants_draw(), "due at 100 ms");
    assert_eq!(
        peek_next_draw_deadline(),
        None,
        "a due deadline is promoted"
    );
    clear_draw_request();
}

#[test]
fn tween_progresses_with_the_virtual_clock() {
    let _g = scoped_virtual(None);
    let mut tween = Tween::new(0.0, 1.0);
    tween.set_target(1.0);
    assert_eq!(tween.tick(), 0.0, "no virtual time has passed");
    advance(Duration::from_millis(500));
    // Ease-out cubic at p = 0.5: 1 - 0.5^3.
    assert_eq!(tween.tick(), 0.875);
    advance(Duration::from_millis(500));
    assert_eq!(tween.tick(), 1.0);
    assert!(!tween.is_animating());
    clear_draw_request();
}

#[test]
fn multi_click_window_follows_the_virtual_clock() {
    let _g = scoped_virtual(None);
    let p = Point::new(10.0, 10.0);

    let mut quick = MultiClickTracker::default();
    assert_eq!(quick.register(p), 1);
    advance(Duration::from_millis(399));
    assert_eq!(
        quick.register(p),
        2,
        "inside the 400 ms window: double click"
    );

    let mut slow = MultiClickTracker::default();
    assert_eq!(slow.register(p), 1);
    advance(Duration::from_millis(400));
    assert_eq!(slow.register(p), 1, "at 400 ms the sequence restarts");
}
