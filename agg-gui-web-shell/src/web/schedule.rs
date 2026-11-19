//! The browser half of the sleeping frame loop: the one
//! `requestAnimationFrame` callback, the one `setTimeout` timer for the next
//! deadline, the `MessageChannel` that runs queued work between frames, and
//! [`wake`], which requests a frame when the loop is asleep.
//!
//! [`crate::frame_schedule`] decides (unit tested natively); this file only
//! carries the decision out. [`install`] registers [`wake`] as the page
//! thread's `agg_gui::frame_demand` hook, so every agg-gui request made
//! outside a tick (a fetch completion, a promise, a timer) wakes the loop,
//! and [`wake_for_work`] as its work hook, so a task a promise woke
//! (`frame_demand::notify_work`) runs in a work run posted on the channel,
//! a task of its own soon after the wake, rather than at the next frame;
//! the shell's own DOM listeners wake it through `mark_dirty` and the input
//! notes in [`super`], and [`super::wake_sources`] adds the browser events
//! that have no other path in (canvas resize, DPR, fullscreen, gamepads).

use std::cell::{Cell, RefCell};

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::frame_schedule::{timer_delay_ms, FrameScheduler, NextTick, Schedule, TimerChange};

type Callback = Closure<dyn FnMut()>;

/// The `MessageChannel` work runs are posted on ([`install_work_channel`]).
struct WorkChannel {
    _channel: web_sys::MessageChannel,
    _receiver: web_sys::MessagePort,
    sender: web_sys::MessagePort,
    _on_message: Callback,
}

thread_local! {
    static SCHEDULER: RefCell<FrameScheduler> = const { RefCell::new(FrameScheduler::new()) };
    static FRAME_CALLBACK: RefCell<Option<Callback>> = const { RefCell::new(None) };
    static TIMER_CALLBACK: RefCell<Option<Callback>> = const { RefCell::new(None) };
    /// The channel work runs are posted on, kept with both its ports (the
    /// receiving one must outlive any garbage collection) and the receiving
    /// port's callback: everything lives as long as the page.
    static WORK_CHANNEL: RefCell<Option<WorkChannel>> = const { RefCell::new(None) };
    /// The armed `setTimeout`'s handle.
    static TIMER_ID: Cell<Option<i32>> = const { Cell::new(None) };
    /// [`request_tick`] was called since the last decision.
    static TICK_REQUESTED: Cell<bool> = const { Cell::new(false) };
    /// A work run could not be posted: the next tick runs the work.
    static TICK_RUNS_WORK: Cell<bool> = const { Cell::new(false) };
}

/// Start the loop: `run` is one tick and `work` one work run, each
/// returning what the loop does next. Requests the first frame.
pub(super) fn install(run: fn() -> NextTick, work: fn() -> NextTick) {
    let frame = Callback::new(move || {
        SCHEDULER.with(|s| s.borrow_mut().begin_tick());
        let next = run();
        let schedule = SCHEDULER.with(|s| s.borrow_mut().end_tick(next));
        carry_out(schedule);
    });
    FRAME_CALLBACK.with(|c| *c.borrow_mut() = Some(frame));
    install_work_channel(work);
    let timer = Callback::new(|| {
        TIMER_ID.with(|t| t.set(None));
        if SCHEDULER.with(|s| s.borrow_mut().timer_fired()) {
            request_frame();
        }
    });
    TIMER_CALLBACK.with(|c| *c.borrow_mut() = Some(timer));
    agg_gui::frame_demand::set_hook(Some(wake));
    agg_gui::frame_demand::set_work_hook(Some(wake_for_work));
    wake();
}

fn carry_out(schedule: Schedule) {
    if schedule.request_frame {
        request_frame();
    }
    match schedule.timer {
        TimerChange::Keep => {}
        TimerChange::Cancel => cancel_timer(),
        TimerChange::Arm(at) => arm_timer(at),
    }
    if schedule.post_work {
        post_work();
    }
}

/// Make the channel work runs are posted on. Without one (no
/// `MessageChannel`), queued work runs with the frames, as every other wake.
fn install_work_channel(work: fn() -> NextTick) {
    let channel = match web_sys::MessageChannel::new() {
        Ok(channel) => channel,
        Err(e) => {
            web_sys::console::warn_2(
                &JsValue::from_str(
                    "agg-gui-web-shell: no MessageChannel; queued work runs with the frames",
                ),
                &e,
            );
            return;
        }
    };
    let on_message = Callback::new(move || {
        SCHEDULER.with(|s| s.borrow_mut().begin_work_run());
        let next = work();
        let schedule = SCHEDULER.with(|s| s.borrow_mut().end_work_run(next));
        carry_out(schedule);
    });
    // Setting `onmessage` starts the port.
    let receiver = channel.port1();
    receiver.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    let sender = channel.port2();
    WORK_CHANNEL.with(|c| {
        *c.borrow_mut() = Some(WorkChannel {
            _channel: channel,
            _receiver: receiver,
            sender,
            _on_message: on_message,
        })
    });
}

/// Queued work that needs no frame woke the page (the page thread's
/// `agg_gui::frame_demand` work hook): post a work run if none is posted.
/// Cheap and safe from anywhere on the page thread, as [`wake`] is.
pub(crate) fn wake_for_work() {
    let go = SCHEDULER.with(|s| s.try_borrow_mut().is_ok_and(|mut s| s.wake_for_work()));
    if go {
        post_work();
    }
}

fn post_work() {
    let posted = WORK_CHANNEL.with(|c| match c.borrow().as_ref() {
        Some(work) => match work.sender.post_message(&JsValue::NULL) {
            Ok(()) => true,
            Err(e) => {
                web_sys::console::error_2(
                    &JsValue::from_str("agg-gui-web-shell: posting a work run failed"),
                    &e,
                );
                false
            }
        },
        None => false,
    });
    if !posted {
        // The next tick runs the work instead; a work run that never comes
        // would leave it waiting for some other wake.
        SCHEDULER.with(|s| s.borrow_mut().work_not_posted());
        TICK_RUNS_WORK.with(|t| t.set(true));
        wake();
    }
}

/// Whether this tick runs the between-frames work a work run could not be
/// posted for (`agg_gui::ui_thread::run_between_frames` in place of the
/// tick's plain drain). Consumes it.
pub(super) fn take_work_for_tick() -> bool {
    TICK_RUNS_WORK.with(|t| t.replace(false))
}

/// Something may want a frame: request one if the loop is asleep. Cheap and
/// safe from anywhere on the page thread, a tick included (there the tick's
/// own decision covers it).
pub(crate) fn wake() {
    // The scheduler is borrowed only inside this file's short sections,
    // none of which call out, so the borrow cannot be held here.
    let go = SCHEDULER.with(|s| s.try_borrow_mut().is_ok_and(|mut s| s.wake()));
    if go {
        request_frame();
    }
}

/// Ask for one more tick without forcing a paint, for state a host polls in
/// `on_tick` / `on_idle` that changed outside the frame loop.
pub fn request_tick() {
    TICK_REQUESTED.with(|t| t.set(true));
    wake();
}

/// Consume a [`request_tick`].
pub(super) fn take_tick_request() -> bool {
    TICK_REQUESTED.with(|t| t.replace(false))
}

fn request_frame() {
    let Some(window) = web_sys::window() else {
        return;
    };
    FRAME_CALLBACK.with(|c| {
        if let Some(cb) = c.borrow().as_ref() {
            if let Err(e) = window.request_animation_frame(cb.as_ref().unchecked_ref()) {
                web_sys::console::error_2(
                    &JsValue::from_str("agg-gui-web-shell: requestAnimationFrame failed"),
                    &e,
                );
            }
        }
    });
}

fn cancel_timer() {
    if let (Some(id), Some(window)) = (TIMER_ID.with(|t| t.take()), web_sys::window()) {
        window.clear_timeout_with_handle(id);
    }
}

fn arm_timer(at: web_time::Instant) {
    cancel_timer();
    let Some(window) = web_sys::window() else {
        return;
    };
    let delay = timer_delay_ms(at, web_time::Instant::now());
    TIMER_CALLBACK.with(|c| {
        let Some(cb) = c.borrow().as_ref().map(|cb| cb.as_ref().clone()) else {
            return;
        };
        match window
            .set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), delay)
        {
            Ok(id) => TIMER_ID.with(|t| t.set(Some(id))),
            Err(e) => {
                web_sys::console::error_2(
                    &JsValue::from_str("agg-gui-web-shell: setTimeout failed; ticking instead"),
                    &e,
                );
                // A deadline nothing will wake for would freeze the page.
                if SCHEDULER.with(|s| s.borrow_mut().timer_fired()) {
                    request_frame();
                }
            }
        }
    });
}
