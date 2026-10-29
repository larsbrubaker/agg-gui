//! Rust-only tests of what an application's own test harness builds on: a
//! runner attached to a driver the caller owns (`AutomationRunner::attach`),
//! a pluggable paint target (`HeadlessPainter`), the background-busy hook and
//! the idle waits it holds open (`wait_for_idle`, `wait_for_pending_ui_work`),
//! and several drivers sharing one thread's clock and UI-thread mark
//! (`src/driver/headless.rs`, `src/driver/thread_clock.rs`).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use agg_gui::{clock, ui_thread, App, Color, Framebuffer, Rect};
use agg_gui_automation::{
    AutomationConfig, AutomationRunner, BackgroundBusy, ClockPolicy, HeadlessDriver,
    HeadlessPainter, ProbeWidget, UiDriver,
};

fn driver(width: u32, height: u32) -> HeadlessDriver {
    let root = ProbeWidget::new("root").with_child(Box::new(
        ProbeWidget::new("red")
            .with_bounds(Rect::new(0.0, 0.0, 10.0, 10.0))
            .with_background(Color::rgb(1.0, 0.0, 0.0)),
    ));
    HeadlessDriver::new(Box::new(root), width, height)
}

/// Counts its paints and captures, sizes, and hands out a solid frame.
#[derive(Clone, Default)]
struct Counts {
    paints: Rc<Cell<u32>>,
    captures: Rc<Cell<u32>>,
    size: Rc<Cell<(u32, u32)>>,
}

struct RecordingPainter {
    counts: Counts,
    keeps_pixels: bool,
}

impl HeadlessPainter for RecordingPainter {
    fn paint(&mut self, _app: &mut App, width: u32, height: u32) {
        self.counts.paints.set(self.counts.paints.get() + 1);
        self.counts.size.set((width, height));
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.counts.size.set((width, height));
    }

    fn capture(&self) -> Option<Framebuffer> {
        if !self.keeps_pixels {
            return None;
        }
        self.counts.captures.set(self.counts.captures.get() + 1);
        let (w, h) = self.counts.size.get();
        let mut fb = Framebuffer::new(w, h);
        fb.pixels_mut().fill(0x7f);
        Some(fb)
    }
}

fn recording(keeps_pixels: bool) -> (Box<dyn HeadlessPainter>, Counts) {
    let counts = Counts::default();
    let painter = RecordingPainter {
        counts: counts.clone(),
        keeps_pixels,
    };
    (Box::new(painter), counts)
}

#[test]
fn rust_only_an_attached_runner_drives_the_callers_driver_and_hands_it_back() {
    let mut runner = AutomationRunner::attach(driver(80, 40), AutomationConfig::default());
    runner.wait_for_draw();
    runner.delay(0.1);
    let screen = runner.get_current_screen();
    assert_eq!((screen.width(), screen.height()), (80, 40));
    assert!(
        screen.pixels().chunks(4).any(|p| p[0] > 200 && p[1] < 50),
        "the default software painter painted the red probe"
    );
    // No run: nothing requires mark_test_complete, and the driver comes back.
    let driver = runner.into_driver();
    assert!(driver.frames() >= 11, "a forced frame and 100 ms of frames");
}

#[test]
fn rust_only_a_plugged_in_painter_paints_every_frame_and_its_capture_is_the_screen() {
    let (painter, counts) = recording(true);
    let mut driver = driver(64, 32).with_painter(painter);
    assert_eq!(counts.size.get(), (64, 32), "told the window size up front");

    assert!(driver.pump(agg_gui_automation::FrameKind::Forced));
    assert_eq!(counts.paints.get(), 1);
    assert_eq!(counts.size.get(), (64, 32));

    // The screen is read out once per painted frame, however often it is asked for.
    let first = driver.current_screen().pixels().to_vec();
    let _ = driver.current_screen();
    assert_eq!(counts.captures.get(), 1);
    assert!(first.iter().all(|&b| b == 0x7f));

    driver.resize(50, 20);
    assert_eq!(counts.size.get(), (50, 20), "a resize reaches the painter");
    assert!(driver.pump(agg_gui_automation::FrameKind::Forced));
    let screen = driver.current_screen();
    assert_eq!((screen.width(), screen.height()), (50, 20));
    assert_eq!(counts.captures.get(), 2, "a new frame is read out again");

    let mut runner = AutomationRunner::attach(driver, AutomationConfig::default());
    let copy = runner.get_current_screen();
    assert_eq!((copy.width(), copy.height()), (50, 20));
    runner.wait_for_draw();
    assert_eq!(counts.paints.get(), 3);
}

#[test]
fn rust_only_a_painter_without_pixels_shows_a_blank_screen_of_the_window_size() {
    let (painter, counts) = recording(false);
    let mut driver = driver(30, 20).with_painter(painter);
    assert!(driver.pump(agg_gui_automation::FrameKind::Forced));
    assert_eq!(counts.paints.get(), 1, "the tree is still painted");
    let screen = driver.current_screen();
    assert_eq!((screen.width(), screen.height()), (30, 20));
    assert!(screen.pixels().iter().all(|&b| b == 0));
}

/// A busy hook that reports busy for its first `n` asks.
fn busy_for(n: usize) -> (BackgroundBusy, Arc<AtomicUsize>) {
    let asks = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&asks);
    let hook = move |_: &App| seen.fetch_add(1, Ordering::SeqCst) < n;
    (Box::new(hook), asks)
}

#[test]
fn rust_only_wait_for_idle_returns_after_a_frame_that_did_nothing() {
    let mut runner = AutomationRunner::attach(driver(40, 40), AutomationConfig::default());
    assert!(runner.wait_for_idle(1.0));
    let frames = runner.driver().frames();
    assert!(runner.wait_for_idle(1.0));
    assert_eq!(
        runner.driver().frames(),
        frames + 1,
        "already idle: one frame"
    );
}

#[test]
fn rust_only_background_busy_keeps_the_idle_waits_pumping_until_it_clears() {
    let mut runner = AutomationRunner::attach(driver(40, 40), AutomationConfig::default());
    runner.wait_for_idle(1.0);
    let (hook, asks) = busy_for(20);
    runner.driver_mut().set_background_busy(hook);
    assert!(runner.driver().background_busy());
    let frames = runner.driver().frames();
    assert!(runner.wait_for_idle(5.0), "idle once the work is done");
    assert!(asks.load(Ordering::SeqCst) > 20);
    assert!(
        runner.driver().frames() - frames >= 19,
        "a frame per busy ask"
    );
    assert!(runner.driver().is_idle());

    let (hook, _) = busy_for(10);
    runner.driver_mut().set_background_busy(hook);
    let frames = runner.driver().frames();
    assert!(runner.wait_for_pending_ui_work(1000));
    assert!(runner.driver().frames() - frames >= 9);
}

#[test]
fn rust_only_background_work_that_never_finishes_runs_the_idle_waits_out() {
    let mut runner = AutomationRunner::attach(driver(40, 40), AutomationConfig::default());
    runner.driver_mut().set_background_busy(Box::new(|_| true));
    assert!(!runner.driver().is_idle());

    let started = clock::now();
    assert!(!runner.wait_for_idle(0.5));
    let waited = clock::since(started).as_secs_f64();
    assert!(
        (0.5..0.52).contains(&waited),
        "waited {waited} s of UI time"
    );

    let started = clock::now();
    assert!(!runner.wait_for_pending_ui_work(250));
    let waited = clock::since(started).as_secs_f64();
    assert!(
        (0.25..0.27).contains(&waited),
        "waited {waited} s of UI time"
    );

    runner.driver_mut().clear_background_busy();
    assert!(runner.wait_for_idle(1.0));
}

#[test]
fn rust_only_wait_for_idle_runs_idle_work_that_queues_more_idle_work() {
    let mut runner = AutomationRunner::attach(driver(40, 40), AutomationConfig::default());
    let ran = Arc::new(AtomicUsize::new(0));
    fn post(ran: Arc<AtomicUsize>) {
        ui_thread::run_on_idle(move || {
            if ran.fetch_add(1, Ordering::SeqCst) + 1 < 5 {
                post(ran);
            }
        });
    }
    post(Arc::clone(&ran));
    assert!(runner.wait_for_idle(1.0));
    assert_eq!(ran.load(Ordering::SeqCst), 5);
}

#[test]
fn rust_only_two_virtual_drivers_share_the_threads_clock_whichever_drops_first() {
    for first_goes_first in [true, false] {
        assert!(
            !clock::is_virtual(),
            "the test thread starts on the real clock"
        );
        let mut first = driver(20, 20);
        let mut second = driver(20, 20);
        assert!(clock::is_virtual());
        let before = clock::now();
        first.pump(agg_gui_automation::FrameKind::Reactive);
        second.pump(agg_gui_automation::FrameKind::Reactive);
        assert_eq!(
            clock::since(before),
            2 * agg_gui_automation::driver::FRAME_INTERVAL
        );

        let (gone, kept) = if first_goes_first {
            (first, second)
        } else {
            (second, first)
        };
        drop(gone);
        assert!(
            clock::is_virtual(),
            "the live driver keeps its virtual clock"
        );
        let now = clock::now();
        let mut kept = kept;
        kept.pump(agg_gui_automation::FrameKind::Reactive);
        assert_eq!(
            clock::since(now),
            agg_gui_automation::driver::FRAME_INTERVAL
        );
        drop(kept);
        assert!(
            !clock::is_virtual(),
            "the last driver restores the real clock"
        );
    }
}

#[test]
fn rust_only_drivers_restore_the_virtual_clock_a_test_set_before_them() {
    let _test_clock = clock::scoped_virtual(None);
    let at = clock::now();
    let mut first = driver(20, 20);
    first.pump(agg_gui_automation::FrameKind::Reactive);
    drop(first);
    assert!(clock::is_virtual());
    assert_eq!(clock::now(), at, "the test's own virtual time is back");
}

#[test]
#[should_panic(expected = "the same ClockPolicy")]
fn rust_only_a_real_clock_driver_cannot_join_a_thread_with_a_virtual_one() {
    let _virtual = driver(20, 20);
    let _real = HeadlessDriver::with_clock(
        Box::new(ProbeWidget::new("root")),
        20,
        20,
        ClockPolicy::Real,
    );
}

#[test]
fn rust_only_a_driver_marks_its_thread_and_leaves_unbinding_to_the_threads_owner() {
    let driver = driver(20, 20);
    assert!(ui_thread::is_ui_thread());
    drop(driver);
    assert!(
        ui_thread::is_ui_thread(),
        "dropping a driver leaves the mark"
    );
    ui_thread::unbind_current_thread();
    assert!(!ui_thread::is_ui_thread());
}
