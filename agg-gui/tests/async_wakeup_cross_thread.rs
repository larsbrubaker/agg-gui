//! Regression coverage for the markdown SVG-badge "wrong scale until any other
//! event" bug.  The image fetch callback (ehttp) runs on a background
//! `std::thread::spawn` worker, so the pre-fix `signal_async_state_change`
//! bumped thread-locals (`NEEDS_DRAW`, `INVALIDATION_EPOCH`,
//! `ASYNC_STATE_EPOCH`) that the main event loop never observed.
//!
//! In production this manifested as: the markdown widget kept polling while
//! the image was `ImageState::Loading` (its `needs_draw` returns `true` for
//! that state), so `wants_draw` stayed `true` and the host kept running
//! frames — but `render_app_frame`'s layout-key cache, which includes
//! `invalidation_epoch`, never saw a bump.  The cache decided no re-layout
//! was needed, so the freshly-decoded SVG inherited the previous layout's
//! placeholder dimensions and was painted squashed.  Any later main-thread
//! event (mouse-move over a scrollbar) bumped `invalidation_epoch` and the
//! next frame's layout pass finally gave the badge its real width.
//!
//! These tests live in an integration-test binary (separate process) so the
//! main queue's wakeup count, which an unbound worker's signal bumps and these
//! unbound test threads read, is isolated from the
//! unit-test binary's other tests, which rely on `wants_draw() == false`
//! after `clear_draw_request()`.

use agg_gui::animation::{
    async_state_epoch, clear_draw_request, invalidation_epoch, signal_async_state_change,
    wants_draw,
};

#[test]
fn signal_async_state_change_propagates_across_threads() {
    // Establish a clean baseline on this (the test runner's) thread.
    clear_draw_request();
    let before_epoch = invalidation_epoch();
    let before_async = async_state_epoch();
    assert!(
        !wants_draw(),
        "baseline: wants_draw must be false after clear_draw_request"
    );

    // Run the signal on a worker thread, just like ehttp's fetch callback.
    std::thread::spawn(|| {
        signal_async_state_change();
    })
    .join()
    .expect("worker thread joined");

    // Main thread must observe all three signals.  Before the fix, every
    // read returned the original value because the worker thread bumped
    // its own thread-locals only.
    assert!(
        invalidation_epoch() != before_epoch,
        "invalidation_epoch must change after a worker-thread \
         signal_async_state_change (render_app_frame's layout-key cache \
         depends on this epoch to re-run layout)"
    );
    assert!(
        async_state_epoch() != before_async,
        "async_state_epoch must change after a worker-thread \
         signal_async_state_change (retained backbuffer caches gate \
         re-raster on this epoch)"
    );
    assert!(
        wants_draw(),
        "wants_draw must return true after a worker-thread \
         signal_async_state_change so the host event loop schedules a frame"
    );
}
