//! Tests for `ui_thread.rs`: what a shell's run between frames does with the
//! queue and the between-frames hooks.

use std::cell::Cell;

std::thread_local! {
    static BETWEEN_FRAMES_RUNS: Cell<u32> = const { Cell::new(0) };
}

fn between_frames_pump() {
    BETWEEN_FRAMES_RUNS.with(|c| c.set(c.get() + 1));
}

/// A shell's work run drains the queue and then runs the between-frames
/// pumps; a frame's own drain leaves those pumps to the frame.
#[test]
fn a_run_between_frames_drains_and_runs_the_between_frames_pumps() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    super::reset_for_tests();
    super::add_between_frames_hook(between_frames_pump);
    super::add_between_frames_hook(between_frames_pump);
    BETWEEN_FRAMES_RUNS.with(|c| c.set(0));
    super::invoke_pending_actions();
    assert_eq!(BETWEEN_FRAMES_RUNS.with(|c| c.get()), 0, "a drain alone");
    let ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&ran);
    super::run_on_idle(move || flag.store(true, Ordering::SeqCst));
    super::run_between_frames();
    assert!(ran.load(Ordering::SeqCst), "the queued action ran");
    assert_eq!(
        BETWEEN_FRAMES_RUNS.with(|c| c.get()),
        1,
        "the pump ran once, added twice"
    );
    super::reset_for_tests();
    crate::animation::clear_draw_request();
}
