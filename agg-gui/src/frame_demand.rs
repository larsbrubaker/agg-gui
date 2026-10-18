//! The frame-demand hook: how a shell whose frame loop sleeps while idle
//! hears that something now wants a frame.
//!
//! Every request a widget or app makes for future work lands in a
//! thread-local or atomic flag that the shell reads on its next loop
//! iteration ([`crate::animation`]'s draw, layout and scheduled-draw
//! requests, a same-thread `ui_thread` post, [`crate::tilt::request_enable`],
//! [`crate::fullscreen::request_toggle`]). A loop that keeps iterating sees
//! them on its own. A loop that has stopped iterating because nothing was
//! pending (`agg-gui-web-shell`'s `requestAnimationFrame` loop, which sleeps
//! so an idle page keeps a phone's CPU asleep) would never look again when
//! the request comes from outside an iteration: a fetch completion, a
//! `setTimeout`, a promise. Each of those request paths calls [`notify`]
//! after setting its flag, and [`notify`] runs the hook the calling thread's
//! shell installed with [`set_hook`].
//!
//! The hook says only "look again"; the shell re-reads the same flags to
//! decide what the next iteration does, so a notification made *during* an
//! iteration (a widget re-arming its caret blink while it paints) costs
//! nothing extra and never turns into a continuous loop. Cross-thread posts
//! reach a parked native loop through [`crate::animation::set_host_waker`]
//! instead; [`notify`] is per thread because the flags it announces are.

use std::cell::Cell;

std::thread_local! {
    static HOOK: Cell<Option<fn()>> = const { Cell::new(None) };
}

/// Install (or, with `None`, remove) the calling thread's frame-demand hook.
/// A shell calls this once at start-up on the thread that runs its loop.
///
/// The hook must be cheap and must not call back into anything that
/// requests a draw (it runs inside `request_draw` and its siblings).
pub fn set_hook(hook: Option<fn()>) {
    HOOK.with(|h| h.set(hook));
}

/// Tell the calling thread's shell that a frame may now be wanted. Called by
/// agg-gui's request paths; an app with its own pending-work flag that the
/// shell polls calls it too.
pub fn notify() {
    if let Some(hook) = HOOK.with(|h| h.get()) {
        hook();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    std::thread_local! {
        static CALLS: Cell<u32> = const { Cell::new(0) };
    }

    fn count() {
        CALLS.with(|c| c.set(c.get() + 1));
    }

    /// Calls the hook saw while `f` ran.
    fn calls_during(f: impl FnOnce()) -> u32 {
        CALLS.with(|c| c.set(0));
        set_hook(Some(count));
        f();
        set_hook(None);
        CALLS.with(|c| c.get())
    }

    #[test]
    fn every_request_path_notifies_the_shell() {
        use crate::animation;
        assert!(calls_during(animation::request_draw) > 0, "request_draw");
        assert!(
            calls_during(|| animation::request_draw_tagged("test")) > 0,
            "request_draw_tagged"
        );
        assert!(
            calls_during(animation::request_draw_without_invalidation) > 0,
            "request_draw_without_invalidation"
        );
        assert!(
            calls_during(animation::request_layout) > 0,
            "request_layout"
        );
        assert!(
            calls_during(|| animation::request_draw_after(Duration::from_secs(5))) > 0,
            "request_draw_after"
        );
        assert!(
            calls_during(|| animation::request_draw_after_tagged(Duration::from_secs(5), "test"))
                > 0,
            "request_draw_after_tagged"
        );
        assert!(
            calls_during(crate::cursor::invalidate_cursor) > 0,
            "invalidate_cursor"
        );
        // Leave the thread's flags as a later test on this thread expects.
        // (`tilt` and `fullscreen` keep process-wide flags, so their own
        // tests check that their requests notify.)
        animation::clear_draw_request();
        let _ = animation::take_layout_request();
    }

    #[test]
    fn a_ui_thread_post_notifies_the_thread_that_drains_it() {
        crate::ui_thread::reset_for_tests();
        let queue = crate::ui_thread::current_queue();
        assert!(calls_during(|| queue.run_on_idle(|| {})) > 0, "run_on_idle");
        assert!(
            calls_during(|| queue.run_on_idle_after(Duration::from_secs(5), || {})) > 0,
            "run_on_idle_after"
        );
        assert!(
            calls_during(crate::animation::signal_async_state_change) > 0,
            "signal_async_state_change"
        );
        queue.clear();
        crate::animation::clear_draw_request();
    }

    #[test]
    fn without_a_hook_requests_are_silent() {
        set_hook(None);
        CALLS.with(|c| c.set(0));
        crate::animation::request_draw();
        assert_eq!(CALLS.with(|c| c.get()), 0);
        crate::animation::clear_draw_request();
    }
}
