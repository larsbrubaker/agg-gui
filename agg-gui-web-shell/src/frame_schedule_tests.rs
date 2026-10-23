//! Tests for `frame_schedule.rs`: the sleep-or-run decision for each wake
//! reason, one frame per wake, and deadlines served by a timer instead of
//! continuous frames.

use super::*;
use std::time::Duration;

/// One reason to keep running: its name and how it changes the demand.
type Reason = (&'static str, fn(&mut LoopDemand));

/// A settled page: painted once, nothing pending.
fn settled(now: Instant) -> LoopDemand {
    LoopDemand {
        can_paint: true,
        continuous: false,
        first_paint_pending: false,
        dirty: false,
        app_wants_draw: false,
        next_draw_deadline: None,
        idle_work_at: None,
        pointer_held: false,
        gamepad_connected: false,
        input_held: false,
        platform_request_pending: false,
        tick_requested: false,
        rebuild_at: None,
        now,
    }
}

#[test]
fn a_settled_page_sleeps() {
    assert_eq!(next_tick(&settled(Instant::now())), NextTick::Sleep);
}

#[test]
fn every_reason_to_run_keeps_the_loop_running() {
    let now = Instant::now();
    let reasons: [Reason; 11] = [
        ("continuous", |d| d.continuous = true),
        ("first paint", |d| d.first_paint_pending = true),
        ("dirty", |d| d.dirty = true),
        ("app wants a draw", |d| d.app_wants_draw = true),
        ("pointer held", |d| d.pointer_held = true),
        ("gamepad connected", |d| d.gamepad_connected = true),
        ("input held for the next tick", |d| d.input_held = true),
        ("tilt or fullscreen request", |d| {
            d.platform_request_pending = true
        }),
        ("host asked for a tick", |d| d.tick_requested = true),
        ("deadline due", |d| d.next_draw_deadline = Some(d.now)),
        ("idle work due", |d| d.idle_work_at = Some(d.now)),
    ];
    for (name, set) in reasons {
        let mut demand = settled(now);
        set(&mut demand);
        assert_eq!(next_tick(&demand), NextTick::Frame, "{name}");
    }
}

#[test]
fn paint_demand_waits_while_the_gpu_is_not_ready() {
    // Before the GPU is up (or once it failed) a dirty flag can't be served;
    // the GPU coming up wakes the loop, so spinning meanwhile is waste.
    let mut d = settled(Instant::now());
    d.can_paint = false;
    d.dirty = true;
    d.app_wants_draw = true;
    d.first_paint_pending = true;
    d.next_draw_deadline = Some(d.now + Duration::from_millis(5));
    assert_eq!(next_tick(&d), NextTick::Sleep);
    // Polling and host ticks don't depend on the GPU.
    d.tick_requested = true;
    assert_eq!(next_tick(&d), NextTick::Frame);
}

#[test]
fn a_future_deadline_arms_a_timer_not_frames() {
    let mut d = settled(Instant::now());
    let blink = d.now + Duration::from_millis(500);
    d.next_draw_deadline = Some(blink);
    assert_eq!(next_tick(&d), NextTick::Timer(blink));

    let mut s = FrameScheduler::new();
    assert!(s.wake());
    s.begin_tick();
    let after = s.end_tick(next_tick(&d));
    assert_eq!(
        after,
        Schedule {
            request_frame: false,
            timer: TimerChange::Arm(blink)
        }
    );
    assert!(s.is_asleep());
}

#[test]
fn a_device_rebuild_backoff_is_a_deadline() {
    let mut d = settled(Instant::now());
    d.can_paint = false;
    let retry = d.now + Duration::from_millis(250);
    d.rebuild_at = Some(retry);
    assert_eq!(next_tick(&d), NextTick::Timer(retry));
    d.can_paint = true;
    let blink = d.now + Duration::from_millis(100);
    d.next_draw_deadline = Some(blink);
    assert_eq!(next_tick(&d), NextTick::Timer(blink), "the earlier one");
}

#[test]
fn a_wake_while_asleep_requests_exactly_one_frame() {
    let mut s = FrameScheduler::new();
    assert!(s.is_asleep());
    assert!(s.wake(), "the first wake requests a frame");
    assert!(!s.wake(), "a second wake shares it");
    assert!(!s.wake());
    s.begin_tick();
    assert!(!s.wake(), "a wake during the tick is the tick's to decide");
    assert_eq!(
        s.end_tick(NextTick::Sleep),
        Schedule {
            request_frame: false,
            timer: TimerChange::Keep
        }
    );
    assert!(s.is_asleep());
    assert!(s.wake(), "asleep again: the next wake requests a frame");
}

#[test]
fn running_frames_request_one_frame_per_tick() {
    let mut s = FrameScheduler::new();
    assert!(s.wake());
    for _ in 0..3 {
        s.begin_tick();
        let after = s.end_tick(NextTick::Frame);
        assert!(after.request_frame);
        assert!(!s.wake(), "the next frame is already requested");
    }
}

#[test]
fn an_unchanged_deadline_keeps_its_timer_and_a_new_one_replaces_it() {
    let now = Instant::now();
    let first = now + Duration::from_millis(500);
    let sooner = now + Duration::from_millis(100);
    let mut s = FrameScheduler::new();
    s.wake();
    s.begin_tick();
    assert_eq!(
        s.end_tick(NextTick::Timer(first)).timer,
        TimerChange::Arm(first)
    );
    // A mouse move wakes the loop; nothing paints and the deadline stands.
    assert!(s.wake());
    s.begin_tick();
    assert_eq!(s.end_tick(NextTick::Timer(first)).timer, TimerChange::Keep);
    s.wake();
    s.begin_tick();
    assert_eq!(
        s.end_tick(NextTick::Timer(sooner)).timer,
        TimerChange::Arm(sooner)
    );
    // Running frames make the timer moot; sleeping drops it.
    s.wake();
    s.begin_tick();
    assert_eq!(s.end_tick(NextTick::Frame).timer, TimerChange::Cancel);
    s.begin_tick();
    s.end_tick(NextTick::Timer(first));
    s.wake();
    s.begin_tick();
    assert_eq!(s.end_tick(NextTick::Sleep).timer, TimerChange::Cancel);
}

#[test]
fn a_fired_timer_wakes_the_loop_once() {
    let now = Instant::now();
    let mut s = FrameScheduler::new();
    s.wake();
    s.begin_tick();
    s.end_tick(NextTick::Timer(now + Duration::from_millis(10)));
    assert!(s.timer_fired(), "the timer requests the frame");
    assert!(!s.wake());
    s.begin_tick();
    // The next decision arms a fresh timer, even for the same instant.
    let again = now + Duration::from_millis(10);
    assert_eq!(
        s.end_tick(NextTick::Timer(again)).timer,
        TimerChange::Arm(again)
    );
}

#[test]
fn the_timer_never_fires_before_its_deadline() {
    let now = Instant::now();
    assert_eq!(timer_delay_ms(now, now), 1);
    assert_eq!(timer_delay_ms(now, now + Duration::from_secs(1)), 1);
    assert_eq!(timer_delay_ms(now + Duration::from_micros(1500), now), 3);
    assert_eq!(timer_delay_ms(now + Duration::from_millis(500), now), 501);
    assert_eq!(
        timer_delay_ms(now + Duration::from_secs(60 * 60 * 24 * 365), now),
        i32::MAX
    );
}

/// Idle-queue work posted during a tick's layout or paint (after that
/// tick's drain) keeps the loop awake even though the paint consumed the
/// draw request the post raised: the decision asks the queue itself.
#[test]
fn idle_work_posted_during_a_paint_keeps_the_loop_awake() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    agg_gui::ui_thread::mark_current_thread_as_ui_thread_with_own_queue();
    agg_gui::ui_thread::invoke_pending_actions(); // the tick's drain
    agg_gui::ui_thread::run_on_idle(|| {}); // posted during layout
    agg_gui::animation::clear_draw_request(); // `App::paint` starts here
    let mut d = settled(agg_gui::clock::now());
    d.idle_work_at = agg_gui::ui_thread::next_due();
    assert_eq!(next_tick(&d), NextTick::Frame);
    agg_gui::ui_thread::invoke_pending_actions();
    assert_eq!(agg_gui::ui_thread::next_due(), None);
    agg_gui::animation::reset_draw_signals();
}

/// A delayed action wakes the loop exactly when it is due: a timer for its
/// time, then a frame (whose drain runs it), with or without a GPU.
#[test]
fn a_delayed_action_wakes_the_loop_exactly_when_due() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let _clock = agg_gui::clock::scoped_virtual(None);
    agg_gui::ui_thread::mark_current_thread_as_ui_thread_with_own_queue();
    let ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&ran);
    let due = agg_gui::clock::now() + Duration::from_millis(400);
    agg_gui::ui_thread::run_on_idle_after(Duration::from_millis(400), move || {
        flag.store(true, Ordering::SeqCst)
    });
    agg_gui::ui_thread::invoke_pending_actions();
    agg_gui::animation::clear_draw_request();
    for can_paint in [true, false] {
        let mut d = settled(agg_gui::clock::now());
        d.can_paint = can_paint;
        d.idle_work_at = agg_gui::ui_thread::next_due();
        assert_eq!(next_tick(&d), NextTick::Timer(due), "can_paint {can_paint}");
    }
    agg_gui::clock::advance(Duration::from_millis(400));
    let mut d = settled(agg_gui::clock::now());
    d.idle_work_at = agg_gui::ui_thread::next_due();
    assert_eq!(next_tick(&d), NextTick::Frame);
    agg_gui::ui_thread::invoke_pending_actions();
    assert!(ran.load(Ordering::SeqCst));
    agg_gui::ui_thread::reset_for_tests();
    agg_gui::animation::reset_draw_signals();
}

std::thread_local! {
    /// The loop state a test's frame-demand hook wakes, as the shell's own
    /// hook (`web::schedule::wake`) wakes the page's.
    static TEST_LOOP: std::cell::RefCell<FrameScheduler> =
        const { std::cell::RefCell::new(FrameScheduler::new()) };
    /// Frames the test hook requested.
    static FRAMES_REQUESTED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn test_wake() {
    if TEST_LOOP.with(|s| s.borrow_mut().wake()) {
        FRAMES_REQUESTED.with(|f| f.set(f.get() + 1));
    }
}

/// One tick of a loop over agg-gui's real draw signals, ending with the
/// decision and what it does to the loop's timer.
fn tick_and_decide() -> (NextTick, TimerChange) {
    TEST_LOOP.with(|s| s.borrow_mut().begin_tick());
    let mut d = settled(agg_gui::clock::now());
    d.app_wants_draw = agg_gui::animation::wants_draw();
    d.next_draw_deadline = agg_gui::animation::peek_next_draw_deadline();
    let next = next_tick(&d);
    let timer = TEST_LOOP.with(|s| s.borrow_mut().end_tick(next)).timer;
    (next, timer)
}

/// Cancelled scheduled draws: withdrawing the earliest wakes the sleeping
/// loop once, whose tick re-arms the timer for the next draw; withdrawing
/// the last lets it sleep with no timer, and the cancelled deadlines never
/// bring a frame.
#[test]
fn the_loop_sleeps_after_the_last_scheduled_draw_is_cancelled() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    agg_gui::animation::reset_draw_signals();
    TEST_LOOP.with(|s| *s.borrow_mut() = FrameScheduler::new());
    FRAMES_REQUESTED.with(|f| f.set(0));
    agg_gui::frame_demand::set_hook(Some(test_wake));

    let watchdog = agg_gui::animation::schedule_draw_after(Duration::from_millis(100));
    let blink = agg_gui::animation::schedule_draw_after(Duration::from_millis(600));
    assert_eq!(
        FRAMES_REQUESTED.with(|f| f.get()),
        1,
        "scheduling woke the loop"
    );
    let (next, timer) = tick_and_decide();
    assert_eq!(next, NextTick::Timer(watchdog.deadline()));
    assert_eq!(timer, TimerChange::Arm(watchdog.deadline()));

    // The guarded work finished early: the watchdog is withdrawn.
    watchdog.cancel();
    assert_eq!(FRAMES_REQUESTED.with(|f| f.get()), 2, "the cancel woke it");
    let (next, timer) = tick_and_decide();
    assert_eq!(next, NextTick::Timer(blink.deadline()));
    assert_eq!(timer, TimerChange::Arm(blink.deadline()), "re-armed later");

    blink.cancel();
    assert_eq!(FRAMES_REQUESTED.with(|f| f.get()), 3);
    let (next, timer) = tick_and_decide();
    assert_eq!(next, NextTick::Sleep);
    assert_eq!(timer, TimerChange::Cancel, "no timer left armed");
    assert!(TEST_LOOP.with(|s| s.borrow().is_asleep()));

    agg_gui::clock::advance(Duration::from_secs(1));
    assert!(
        !agg_gui::animation::wants_draw(),
        "the withdrawn deadlines never ask for a frame"
    );
    assert_eq!(
        FRAMES_REQUESTED.with(|f| f.get()),
        3,
        "and never wake the loop"
    );
    agg_gui::frame_demand::set_hook(None);
}
