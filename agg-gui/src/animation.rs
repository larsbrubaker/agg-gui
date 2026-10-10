//! Thread-local draw-request and invalidation signals.
//!
//! Two independent channels feed the host's event loop:
//!
//! 1. **Immediate draw request** — [`request_draw`] / [`wants_draw`].  Any
//!    widget whose visual output just changed calls `request_draw()`; the next
//!    iteration of the host loop draws a frame and clears the flag.  The same
//!    call advances [`invalidation_epoch`], letting event dispatch dirty the
//!    affected retained ancestor path even when the event bubbles as ignored.
//!
//! 2. **Scheduled draw** — [`request_draw_after`] /
//!    [`peek_next_draw_deadline`].  A
//!    widget that needs a draw *at a future time* (text-cursor blink,
//!    tooltip delay) calls `request_draw_after(Duration)`; the host's
//!    loop goes to sleep with `ControlFlow::WaitUntil(that_instant)` and
//!    draws when the deadline fires.  Successive calls keep the EARLIEST
//!    deadline.
//!
//! The scheduled channel is read **non-destructively** via
//! [`peek_next_draw_deadline`]: a host re-arms its `WaitUntil` from the same
//! pending deadline on every idle iteration, so an intervening event that
//! does not itself repaint can no longer strand the wake (the reactive-host
//! "lost wakeup" that stalled tooltips, cursor blink, and scrollbar fades).
//! Once a pending deadline comes due, [`wants_draw`] observes it, clears the
//! cell, and raises the immediate-draw flag — a due deadline is deliberately
//! made indistinguishable from a plain [`request_draw`], upholding the
//! framework invariant that *anything needing a future draw eventually makes
//! `wants_draw()` true by itself*.  Consumers re-arm during the ensuing paint,
//! which keeps recurring timers alive.
//!
//! The host loop draws iff `wants_draw()` (now inclusive of due deadlines).
//! Between draws it idles with `WaitUntil(peek_next_draw_deadline())`; no
//! frames are drawn while nothing has changed.
//!
//! 3. **Layout request** — [`request_layout`] / [`layout_requested`].  A
//!    widget with more work for its *next layout pass* (a model executor
//!    pumped from `layout`, a measurement that settles over several passes)
//!    calls `request_layout()`.  Unlike the immediate draw flag, the request
//!    survives `App::paint`'s [`clear_draw_request`]: only `App::layout`
//!    consumes it ([`take_layout_request`], at the *start* of the pass, so a
//!    request made during layout or paint stays pending for the following
//!    frame).  [`wants_draw`] reports `true` while one is pending, and the
//!    shells force `needs_layout` for that frame.

use std::cell::Cell;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_time::Instant;

// ── Draw-request provenance trace ─────────────────────────────────────────────
//
// A thread-local ring buffer of `&'static str` reason tags, appended by the
// `*_tagged` request helpers below.  It exists to answer one question that a
// stack-free thread-local signal otherwise makes impossible: *who* keeps the
// reactive host awake when the app should be idle?  demo-ui's all-closed
// quiescence guard drains it to name culprits; the close-phase guard reads
// `draw_trace_log` (fed by the same helpers) by cursor instead.
//
// Cost: recording is compiled out entirely in release (`debug_assertions`
// off) so shipping hosts pay nothing.  In debug/test builds each tagged
// request pushes one pointer-sized tag into a small capped `Vec`.

#[cfg(debug_assertions)]
std::thread_local! {
    static DRAW_TRACE: std::cell::RefCell<Vec<&'static str>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Cap on retained trace tags — a soft ring buffer: oldest tags drop once the
/// cap is hit so a long-running session can't grow the buffer unbounded.
#[cfg(debug_assertions)]
const DRAW_TRACE_CAP: usize = 512;

#[cfg(debug_assertions)]
fn record_draw_trace(reason: &'static str) {
    DRAW_TRACE.with(|t| {
        let mut t = t.borrow_mut();
        if t.len() >= DRAW_TRACE_CAP {
            t.remove(0);
        }
        t.push(reason);
    });
}

#[cfg(not(debug_assertions))]
#[inline(always)]
fn record_draw_trace(_reason: &'static str) {}

/// Drain and return the recorded draw-request provenance tags (debug builds
/// only; always empty in release).  Tests call this after driving frames to
/// name whatever kept the reactive host awake.
#[doc(hidden)]
pub fn drain_draw_trace() -> Vec<&'static str> {
    #[cfg(debug_assertions)]
    {
        DRAW_TRACE.with(|t| std::mem::take(&mut *t.borrow_mut()))
    }
    #[cfg(not(debug_assertions))]
    {
        Vec::new()
    }
}

/// [`request_draw`] with a provenance tag — see the trace module docs.  Prefer
/// this from library call sites so the quiescence guard can attribute wakeups.
pub fn request_draw_tagged(reason: &'static str) {
    record_draw_trace(reason);
    crate::draw_trace_log::record(reason);
    set_draw_request();
}

/// [`request_draw_after`] with a provenance tag — see the trace module docs.
pub fn request_draw_after_tagged(delay: Duration, reason: &'static str) {
    record_draw_trace(reason);
    crate::draw_trace_log::record(reason);
    request_draw_after(delay);
}

std::thread_local! {
    static NEEDS_DRAW:        Cell<bool>            = Cell::new(false);
    static NEXT_DRAW_AT:      Cell<Option<Instant>> = Cell::new(None);
    /// Pending [`request_layout`]; consumed only by `App::layout`.
    static LAYOUT_REQUESTED:  Cell<bool>            = const { Cell::new(false) };
    static INVALIDATION_EPOCH: Cell<u64>             = Cell::new(0);
    /// Bumped whenever an async source (image fetch + decode, font
    /// load, etc.) finishes outside the event-dispatch path.  Retained
    /// backbuffers (Window FBOs, in-process bitmap caches) compare
    /// their stored value against this epoch on each paint and force
    /// a re-raster on mismatch — there is no widget reference at the
    /// callback site to walk the ancestor chain via the usual
    /// `mark_dirty` route, so without this signal a freshly-decoded
    /// image draws into the placeholder-sized rect the previous
    /// layout reserved (the user-visible "wrong scale on first
    /// frame" bug).
    static ASYNC_STATE_EPOCH: Cell<u64> = Cell::new(0);
    /// Per-thread snapshot of its queue's wakeup count last observed by
    /// [`pump_async_wakeup`].  When the count differs from this, the
    /// current thread's [`NEEDS_DRAW`], [`INVALIDATION_EPOCH`] and
    /// [`ASYNC_STATE_EPOCH`] are bumped — see the comment above the host
    /// waker for why this indirection is required.
    static LAST_SEEN_ASYNC_WAKEUP: Cell<u64> = Cell::new(0);
    /// Monotonic counter bumped once per pointer press that reaches the
    /// widget tree (see [`bump_pointer_press_epoch`]).  A widget that runs
    /// its own multi-click gesture but no longer sees every press —
    /// [`Scene`](crate::widgets::Scene), whose hosted children consume
    /// their own presses before they can bubble — reads this to tell
    /// whether an *intervening* press (e.g. a click on a hosted button)
    /// happened between two of its own background clicks, so a
    /// background double-click that straddles a child interaction does
    /// not falsely fire.
    static POINTER_PRESS_EPOCH: Cell<u64> = Cell::new(0);
}

/// Advance the pointer-press epoch.  Called by [`App`](crate::App) once per
/// pointer press that is about to be routed into the widget tree.
pub fn bump_pointer_press_epoch() {
    POINTER_PRESS_EPOCH.with(|c| c.set(c.get().wrapping_add(1)));
}

/// Current pointer-press epoch — see [`bump_pointer_press_epoch`].  Two
/// presses are *consecutive* (nothing pressed in between) exactly when their
/// observed epochs differ by one.
pub fn pointer_press_epoch() -> u64 {
    POINTER_PRESS_EPOCH.with(|c| c.get())
}

// Cross-thread wakeups are counted per UI-thread queue
// (`crate::ui_thread::UiQueue::signal_async_state_change`): a worker's bump is
// invisible to the UI thread's thread-locals, so the UI thread merges its
// queue's count into its own epochs on every `wants_draw` /
// `invalidation_epoch` / `async_state_epoch` read — see [`pump_async_wakeup`].

// ── Host waker ───────────────────────────────────────────────────────────────
//
// Bumping a queue's wakeup counter is only half the story for a *reactive* host.
// The main thread merges the bump in `pump_async_wakeup`, but that runs only
// when something already made the host read `wants_draw()` / an epoch.  A host
// parked in winit's `ControlFlow::Wait` (or `WaitUntil` with a far deadline) is
// not executing at all, so a worker-thread `signal_async_state_change` would
// sit unobserved until an unrelated OS event happened to wake the loop.
// Consumers otherwise have to paper over this with a per-frame keep-alive
// repaint, which burns a core to stay idle.
//
// The optional host waker closes that gap: the host installs a cheap,
// thread-safe nudge (typically `EventLoopProxy::send_event`) once at startup,
// and `signal_async_state_change` calls it right after the atomic bump so the
// woken loop is guaranteed to observe the new counter value.
type HostWaker = Arc<dyn Fn() + Send + Sync>;

/// Process-global waker slot.  Locked only to clone the `Arc` out; the waker
/// itself always runs with the lock released (see [`signal_async_state_change`]).
static HOST_WAKER: Mutex<Option<HostWaker>> = Mutex::new(None);

/// Install (or replace) the process-global host waker.
///
/// Call this once from a reactive host's startup path, passing a closure that
/// nudges the event loop awake — on winit that is
/// `move || { let _ = proxy.send_event(UserEvent::Wake); }`.  Whenever any
/// thread calls [`signal_async_state_change`], the waker fires *after* the
/// cross-thread counter has been bumped, so the host is guaranteed to see the
/// pending wakeup on its next `wants_draw()` read.
///
/// Requirements on `waker`:
/// * **Cheap** — it runs inline on whatever worker thread finished an async
///   load; do no real work in it, just signal.
/// * **Thread-safe and non-reentrant** — it may be called from any thread and
///   must not call back into `signal_async_state_change`.
/// * **Failure-tolerant** — a closed event loop should be ignored (drop the
///   `send_event` error) rather than panicking.
///
/// Hosts that already pump every tick — the wasm `requestAnimationFrame` loop,
/// game-style continuous redraw hosts, and the headless test harnesses — do not
/// need a waker; they reach `pump_async_wakeup` on their own schedule.
pub fn set_host_waker(waker: impl Fn() + Send + Sync + 'static) {
    let waker: HostWaker = Arc::new(waker);
    match HOST_WAKER.lock() {
        Ok(mut slot) => *slot = Some(waker),
        // Poison-tolerant, like the rest of this module's best-effort signals:
        // a panic elsewhere must not permanently disable host wakeups.
        Err(poisoned) => *poisoned.into_inner() = Some(waker),
    }
}

/// Remove any installed host waker, restoring the plain counter-only behaviour.
///
/// Hosts call this on shutdown so a dangling `EventLoopProxy` isn't retained;
/// tests call it to reset this process-global slot between cases.
pub fn clear_host_waker() {
    match HOST_WAKER.lock() {
        Ok(mut slot) => *slot = None,
        Err(poisoned) => *poisoned.into_inner() = None,
    }
}

/// Clone the installed waker out of its lock, if any.  The clone exists so the
/// call site can release the lock before invoking arbitrary host code.
fn host_waker() -> Option<HostWaker> {
    match HOST_WAKER.lock() {
        Ok(slot) => slot.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Merge any pending cross-thread async-wakeup bumps into the calling
/// thread's draw/invalidation/async-state state.
///
/// Without this, an ehttp callback completing on a background thread
/// bumps thread-locals the main event loop never reads — the markdown
/// SVG-badge "wrong scale until any other event" bug, where the loop
/// keeps polling (`needs_draw=true` while `ImageState::Loading`) but
/// `invalidation_epoch` never changes, so `render_app_frame` skips
/// the layout pass and paints the freshly-decoded SVG into the
/// previous layout's placeholder rect.
fn pump_async_wakeup() {
    let current = crate::ui_thread::current_wakeups();
    let changed = LAST_SEEN_ASYNC_WAKEUP.with(|c| {
        let prev = c.get();
        if prev == current {
            false
        } else {
            c.set(current);
            true
        }
    });
    if changed {
        bump_local_async_state();
    }
}

/// Raise the calling thread's draw request and advance its invalidation and
/// async-state epochs: what a merged wakeup does.
pub(crate) fn bump_local_async_state() {
    NEEDS_DRAW.with(|c| c.set(true));
    INVALIDATION_EPOCH.with(|c| c.set(c.get().wrapping_add(1)));
    ASYNC_STATE_EPOCH.with(|c| c.set(c.get().wrapping_add(1)));
}

/// Run the installed host waker, if any, with its lock released.
pub(crate) fn wake_host() {
    if let Some(waker) = host_waker() {
        waker();
    }
}

/// Request that the host schedule another draw as soon as possible.
///
/// **This is the right default for every widget state mutation that affects
/// visual output.**  Calling it from inside an `on_event` handler advances
/// [`invalidation_epoch`]; `dispatch_event` reads that epoch before/after
/// delivery and automatically calls `mark_dirty` up the ancestor path when
/// it sees a bump — so a retained ancestor's backbuffer cache invalidates
/// without the widget needing to know about that ancestor at all.
///
/// Without the epoch bump, a `Widget::on_event` that returns `Ignored` (the
/// common case for `MouseMove`) leaves the ancestor cache thinking
/// "nothing changed", and the next frame composites a stale bitmap.  Hover
/// effects, focus rings, and any other appearance change driven by event
/// state ALL need this hook.
///
/// Reach for [`request_draw_without_invalidation`] only when you're certain
/// no retained widget's *content* changed — overlays, position-only
/// translations, and similar.  When in doubt, use `request_draw`.
pub fn request_draw() {
    crate::draw_trace_log::record(crate::draw_trace_log::UNTAGGED_DRAW_REQUEST);
    set_draw_request();
}

/// The body of [`request_draw`] without logging it (the tagged variant logs
/// its own tag instead).
fn set_draw_request() {
    NEEDS_DRAW.with(|c| c.set(true));
    INVALIDATION_EPOCH.with(|c| c.set(c.get().wrapping_add(1)));
}

/// Request a frame **without** advancing [`invalidation_epoch`].
///
/// `dispatch_event` won't mark retained ancestors dirty for this call, so
/// any widget that drew its previous frame into a backbuffer cache will
/// composite the cached bitmap unchanged.  Use this **only** when:
///
/// * The change lives in an app-level overlay that paints fresh every
///   frame outside any retained subtree (inspector hover rectangle, popup
///   menus rendered via `paint_global_overlay`, scroll-fade decorations).
/// * The change is position-only — a window drag-move, where the cached
///   content is reused at a translated origin (see `Window::on_event` for
///   the canonical example).
///
/// **Do NOT call this from a widget that mutated its own state and expects
/// the next paint to reflect it.**  That's [`request_draw`]'s job.  Hover
/// indices, focus changes, animation ticks, button-press states — anything
/// where the *content* of a retained widget differs from the cached
/// bitmap — must call `request_draw` so the cache invalidates.  The
/// `MenuBar` hover regression in `widgets/menu/widget/tests_2.rs` exists
/// precisely because this distinction was missed once already.
pub fn request_draw_without_invalidation() {
    crate::draw_trace_log::record(crate::draw_trace_log::UNTAGGED_DRAW_REQUEST);
    NEEDS_DRAW.with(|c| c.set(true));
}

/// Request another layout **and** paint pass.
///
/// For a widget whose next `layout` has more work to do — mattercad's design
/// page pumps its model executor from `layout` while a rebuild is running, for
/// example.  A plain [`request_draw`] made during layout is dropped by the
/// [`clear_draw_request`] at the top of `App::paint`, and a `needs_draw()`
/// that stays `true` repaints every frame without ever laying out again.
///
/// The request stays pending until the next `App::layout` begins (see
/// [`take_layout_request`]), so calling it from `layout`, `paint` or an event
/// handler always yields one more laid-out frame.  While pending,
/// [`wants_draw`] returns `true` and the shells (`agg-gui-shell`,
/// `agg-gui-web-shell`, demo-wgpu's `render_app_frame`) lay out the frame
/// regardless of their layout-skip key.  It also behaves as a [`request_draw`]
/// (advancing [`invalidation_epoch`]), so a host that only keys layout on the
/// epoch re-lays out too.  Re-request on each pass for as long as work
/// remains; the loop goes idle once a pass makes no request.
pub fn request_layout() {
    LAYOUT_REQUESTED.with(|c| c.set(true));
    request_draw_tagged("animation.request_layout");
}

/// Non-destructive read of the pending [`request_layout`] flag.  Hosts OR this
/// into their "does this frame need layout" decision.
pub fn layout_requested() -> bool {
    LAYOUT_REQUESTED.with(|c| c.get())
}

/// Consume the pending [`request_layout`], returning whether one was pending.
/// `App::layout` calls this before laying out the tree, so a request made
/// during that same pass remains pending for the next frame.
pub fn take_layout_request() -> bool {
    LAYOUT_REQUESTED.with(|c| c.replace(false))
}

/// Non-destructive read of the immediate-draw signal, *plus* the promotion
/// point for a due scheduled deadline.  Hosts call this after drawing to
/// decide control-flow for the next loop iteration.
///
/// Pumps any pending cross-thread async-wakeup bumps first, so a fetch
/// callback that finished on a worker thread between frames is reflected
/// in the result.
///
/// If no immediate draw is pending but a [`request_draw_after`] deadline has
/// come due on the UI clock (`clock::now() >= deadline`), this clears the scheduled cell and
/// raises [`NEEDS_DRAW`], returning `true`.  That makes a due deadline
/// indistinguishable from an immediate [`request_draw`]: the normal
/// request_draw → paint → [`clear_draw_request`] cycle then applies, and
/// consumers re-arm their next deadline during that paint (so recurring timers
/// stay alive).  This is what lets a purely reactive host serve scheduled
/// draws without relying on a `WaitUntil` surviving intact — see the module
/// docs on the lost-wakeup fix.
pub fn wants_draw() -> bool {
    pump_async_wakeup();
    if NEEDS_DRAW.with(|c| c.get()) || layout_requested() {
        return true;
    }
    let due = NEXT_DRAW_AT.with(|c| match c.get() {
        Some(when) if crate::clock::now() >= when => {
            c.set(None);
            true
        }
        _ => false,
    });
    if due {
        NEEDS_DRAW.with(|c| c.set(true));
    }
    due
}

/// Monotonic draw-request epoch used to detect visual changes during dispatch.
///
/// Pumps cross-thread wakeups first so a background-thread
/// [`signal_async_state_change`] is observed here on the next read,
/// causing layout-key caches keyed on this epoch to re-layout.
pub fn invalidation_epoch() -> u64 {
    pump_async_wakeup();
    INVALIDATION_EPOCH.with(|c| c.get())
}

/// Note that an async-side state change happened (image loader finished,
/// font loaded, etc.).  Safe to call from any thread: it wakes the UI thread
/// whose queue the caller posts to ([`crate::ui_thread::current_queue`]) —
/// the caller itself on a UI thread, the main queue's owner from an unbound
/// worker — which observes it on its next `wants_draw` /
/// `invalidation_epoch` / `async_state_epoch` read.  A worker serving another
/// UI thread signals through that thread's queue handle
/// ([`crate::ui_thread::UiQueue::signal_async_state_change`]).  Other UI
/// threads are not woken, so parallel headless tests stay independent.
///
/// The bump must reach the UI thread from background threads (ehttp spawns
/// its own `std::thread`); a thread-local-only bump once left the main loop's
/// layout-key cache skipping the layout pass that gives freshly-decoded SVG
/// badges their natural dimensions (the "wrong scale until any other event"
/// bug).
///
/// If a host waker is installed ([`set_host_waker`]), it is invoked after the
/// cross-thread bump so a reactive host parked in `ControlFlow::Wait` wakes and
/// observes the already-published count.
pub fn signal_async_state_change() {
    crate::ui_thread::current_queue().signal_async_state_change();
}

/// Current async-state epoch.  Backbuffer caches store this and force
/// a re-raster when it doesn't match.
///
/// Pumps cross-thread wakeups first so a worker-thread
/// [`signal_async_state_change`] surfaces on the next read.
pub fn async_state_epoch() -> u64 {
    pump_async_wakeup();
    ASYNC_STATE_EPOCH.with(|c| c.get())
}

/// Reset the per-frame draw flags.  The `App::paint` entry point calls
/// this before delegating to the root widget so each frame starts fresh —
/// widgets that still need a draw (animation in flight, focus blink, etc.)
/// must re-arm during their draw, otherwise the loop goes idle.
///
/// Pending cross-thread async wakeups are *merged* first (advancing
/// [`invalidation_epoch`] and [`async_state_epoch`]) and only then is the
/// immediate flag cleared.  Merging — rather than just marking the counter
/// seen — means a worker-thread [`signal_async_state_change`] landing after
/// the host's last `wants_draw()` but before `App::paint` still reaches that
/// paint's async-state dirty walk instead of being silently swallowed.
/// Clearing the flag afterwards keeps the old guarantee that a stale bump
/// cannot reappear on the next `wants_draw` read (parallel tests calling
/// [`signal_async_state_change`] must not leak wakeups into unrelated tests
/// that rely on `wants_draw()` returning `false` after a clear).
///
/// A pending [`request_layout`] is deliberately left alone: only `App::layout`
/// consumes it.
pub fn clear_draw_request() {
    pump_async_wakeup();
    NEEDS_DRAW.with(|c| c.set(false));
    NEXT_DRAW_AT.with(|c| c.set(None));
}

/// Clear **only** the immediate draw request ([`request_draw`]'s flag).
///
/// For a host that tried to draw and could not — the surface refused the
/// frame (swap chain backing off after a failed configure, window occluded).
/// That frame never reaches `App::paint`, the only other place the flag is
/// cleared, so left set it keeps `wants_draw()` true and a reactive host
/// spinning in `ControlFlow::Poll`. The host has already arranged its own
/// wake for the retry.
///
/// Unlike [`clear_draw_request`], this leaves the scheduled deadline
/// ([`request_draw_after`]) in place — it may be the very wake that retries
/// the frame.  It also does not pump pending cross-thread
/// [`signal_async_state_change`] bumps; they merge on the next read, where
/// they both advance [`async_state_epoch`] and raise a draw request (whereas
/// `clear_draw_request` merges them into the epochs and then clears the
/// resulting request, since the paint it precedes consumes them).
pub fn clear_immediate_draw_request() {
    NEEDS_DRAW.with(|c| c.set(false));
}

/// Schedule a future draw.  Keeps the EARLIEST pending deadline, so multiple
/// widgets asking for different delays will all be served by the soonest one
/// (each widget re-arms its own deadline on the next draw anyway).
pub fn request_draw_after(delay: Duration) {
    let when = crate::clock::now() + delay;
    NEXT_DRAW_AT.with(|c| match c.get() {
        Some(existing) if existing <= when => {}
        _ => c.set(Some(when)),
    });
}

/// Side-effect-free snapshot of the two draw signals for diagnostics.
///
/// Returns `(immediate_flag, next_deadline)` read straight from the
/// thread-local cells.  Unlike [`wants_draw`], this does **not** pump
/// cross-thread async wakeups and does **not** promote or clear a due
/// deadline — reading it can never perturb the very runaway a caller is
/// trying to capture.  Used by [`crate::debug_draw_report`].
#[doc(hidden)]
pub fn peek_draw_signals() -> (bool, Option<Instant>) {
    let flag = NEEDS_DRAW.with(|c| c.get());
    let deadline = NEXT_DRAW_AT.with(|c| c.get());
    (flag, deadline)
}

/// Non-destructive read of the earliest pending scheduled-draw deadline.
///
/// Hosts arm `ControlFlow::WaitUntil(t)` from this on every idle iteration.
/// Because it does **not** clear the cell, re-arming is idempotent: an
/// intervening event that does not itself repaint cannot strand the scheduled
/// wake (the reactive-host lost-wakeup bug).  The cell is cleared only when
/// the deadline actually comes due — [`wants_draw`] promotes it to an
/// immediate draw — or by [`clear_draw_request`] at the start of a paint,
/// after which consumers re-arm.
///
/// The deadline is in UI-clock time ([`crate::clock`]); with the real clock
/// (every shell) that is the wall clock.
pub fn peek_next_draw_deadline() -> Option<Instant> {
    NEXT_DRAW_AT.with(|c| c.get())
}

// ── Tween ────────────────────────────────────────────────────────────────────
//
// Small reusable time-based interpolator for widgets that want a smooth
// transition between two scalar states (hover ↔ dormant, off ↔ on, etc.).
// Ease-out cubic; reversal preserves the current value so rapid toggles
// don't snap.  Requests a draw automatically while in flight.

/// Smooth scalar tween between `0.0` and `1.0` (or any pair of values the
/// caller interprets).  Drives animations such as the scroll-bar hover
/// expansion and toggle-switch on/off slide.
#[derive(Clone, Copy)]
pub struct Tween {
    current: f64,
    start_value: f64,
    target: f64,
    start_time: Option<Instant>,
    duration: f64,
}

impl Tween {
    /// New tween that starts at `initial` with the same value as its target
    /// (no animation in flight).
    pub const fn new(initial: f64, duration_secs: f64) -> Self {
        Self {
            current: initial,
            start_value: initial,
            target: initial,
            start_time: None,
            duration: duration_secs,
        }
    }

    /// Update the target.  If it differs from the current target, re-anchors
    /// the animation at the current interpolated value so reversals are smooth.
    ///
    /// Widgets that own a `Tween` must also report `tween.is_animating()` from
    /// `Widget::needs_draw()` so retained parents repaint every frame until
    /// the tween settles. [`Tween::tick`] is the draw-request point; `set_target`
    /// intentionally does not invalidate because many widgets retarget from
    /// paint while synchronizing with external state.
    pub fn set_target(&mut self, new_target: f64) {
        if (self.target - new_target).abs() > 1e-9 {
            self.start_value = self.current;
            self.target = new_target;
            self.start_time = Some(crate::clock::now());
        }
    }

    /// Advance the animation by elapsed UI-clock time (`crate::clock`) and
    /// return the new interpolated value.  Ease-out cubic.  While in flight
    /// this also requests a draw tagged `"animation.tween"` (see
    /// [`request_draw_tagged`]) so the host keeps drawing frames until
    /// completion, and a running tween is named in the provenance trace.
    pub fn tick(&mut self) -> f64 {
        if let Some(start) = self.start_time {
            let elapsed = crate::clock::since(start).as_secs_f64();
            let p = (elapsed / self.duration).min(1.0);
            let eased = 1.0 - (1.0 - p).powi(3);
            self.current = self.start_value + (self.target - self.start_value) * eased;
            if p >= 1.0 {
                self.current = self.target;
                self.start_time = None;
            } else {
                request_draw_tagged("animation.tween");
            }
        }
        self.current
    }

    /// Current interpolated value without advancing.
    pub fn value(&self) -> f64 {
        self.current
    }

    /// Where the tween is animating *towards* — i.e. the value last
    /// passed to [`Self::set_target`].  Lets tests assert intent
    /// (`request_lift(0.0)` was called) without waiting for the
    /// animation to settle, which is otherwise wall-clock-dependent.
    pub fn target(&self) -> f64 {
        self.target
    }

    /// Whether the tween still needs frames to reach its target.
    pub fn is_animating(&self) -> bool {
        self.start_time.is_some()
    }
}

impl Default for Tween {
    fn default() -> Self {
        Self::new(0.0, 0.12)
    }
}

#[cfg(test)]
mod host_waker_tests {
    //! Coverage for the reactive-host waker hook: a worker-thread
    //! [`signal_async_state_change`] must be able to nudge a host parked in
    //! `ControlFlow::Wait`.
    //!
    //! `HOST_WAKER` and the main queue's wakeup count (which these unbound test
    //! threads read) are process-global, and other tests in this crate call
    //! `signal_async_state_change` concurrently — so
    //! these tests serialize against each other with a local mutex, count
    //! *their own* invocations rather than reading the global counter after
    //! the fact, and assert with `>=` where a foreign signal could add more.
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Serializes only these tests; unrelated tests may still signal in
    /// parallel, which is why every assertion below tolerates extra fires.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn waker_fires_on_signal() {
        let _guard = serial();
        let fires = Arc::new(AtomicU64::new(0));
        let seen = Arc::clone(&fires);
        set_host_waker(move || {
            seen.fetch_add(1, Ordering::AcqRel);
        });

        signal_async_state_change();
        clear_host_waker();

        assert!(
            fires.load(Ordering::Acquire) >= 1,
            "installed waker must run when any thread signals an async change"
        );
    }

    #[test]
    fn waker_sees_counter_already_bumped() {
        let _guard = serial();
        let before = crate::ui_thread::current_wakeups();
        let observed = Arc::new(AtomicU64::new(0));
        let sink = Arc::clone(&observed);
        set_host_waker(move || {
            // Snapshot from inside the waker: the host is woken only after the
            // bump is published, otherwise it would park again having seen
            // nothing.
            sink.store(crate::ui_thread::current_wakeups(), Ordering::Release);
        });

        signal_async_state_change();
        clear_host_waker();

        assert!(
            observed.load(Ordering::Acquire) > before,
            "the counter must be bumped BEFORE the waker runs"
        );
    }

    #[test]
    fn replacing_the_waker_retires_the_old_one() {
        let _guard = serial();
        let old_fires = Arc::new(AtomicU64::new(0));
        let old_sink = Arc::clone(&old_fires);
        set_host_waker(move || {
            old_sink.fetch_add(1, Ordering::AcqRel);
        });

        let new_fires = Arc::new(AtomicU64::new(0));
        let new_sink = Arc::clone(&new_fires);
        set_host_waker(move || {
            new_sink.fetch_add(1, Ordering::AcqRel);
        });
        let old_baseline = old_fires.load(Ordering::Acquire);

        signal_async_state_change();
        clear_host_waker();

        assert!(
            new_fires.load(Ordering::Acquire) >= 1,
            "the replacement waker runs"
        );
        assert_eq!(
            old_fires.load(Ordering::Acquire),
            old_baseline,
            "the replaced waker no longer runs"
        );
    }

    #[test]
    fn clear_stops_the_waker() {
        let _guard = serial();
        let fires = Arc::new(AtomicU64::new(0));
        let sink = Arc::clone(&fires);
        set_host_waker(move || {
            sink.fetch_add(1, Ordering::AcqRel);
        });
        clear_host_waker();
        let baseline = fires.load(Ordering::Acquire);

        signal_async_state_change();

        assert_eq!(
            fires.load(Ordering::Acquire),
            baseline,
            "a cleared waker must not run again"
        );
    }

    #[test]
    fn signal_from_a_worker_thread_wakes_the_host() {
        let _guard = serial();
        let fires = Arc::new(AtomicU64::new(0));
        let sink = Arc::clone(&fires);
        set_host_waker(move || {
            sink.fetch_add(1, Ordering::AcqRel);
        });

        // The whole point of the hook: the signal originates off the main
        // thread, where thread-local bumps are invisible to the host.
        std::thread::spawn(signal_async_state_change)
            .join()
            .expect("worker thread signalled without panicking");
        clear_host_waker();

        assert!(
            fires.load(Ordering::Acquire) >= 1,
            "a worker-thread signal must reach the host waker"
        );
    }

    #[test]
    fn signalling_without_a_waker_is_a_no_op() {
        let _guard = serial();
        clear_host_waker();
        // Must not panic and must still publish the cross-thread bump.
        let before = crate::ui_thread::current_wakeups();
        signal_async_state_change();
        assert!(
            crate::ui_thread::current_wakeups() > before,
            "counter-only behaviour is unchanged when no waker is installed"
        );
    }
}

#[cfg(test)]
#[path = "animation_scheduled_draw_tests.rs"]
mod scheduled_draw_tests;

#[cfg(test)]
#[path = "animation_layout_request_tests.rs"]
mod layout_request_tests;
