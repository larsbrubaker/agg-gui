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

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use web_time::Instant;

// ── Draw-request provenance trace ─────────────────────────────────────────────
//
// A thread-local ring buffer of `&'static str` reason tags, appended by the
// `*_tagged` request helpers below.  It exists to answer one question that a
// stack-free thread-local signal otherwise makes impossible: *who* keeps the
// reactive host awake when the app should be idle?  When the quiescence
// regression guard (demo-ui) finds the app still wants a draw after settling,
// it drains this buffer and names the culprits in its failure message.
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
    request_draw();
}

/// [`request_draw_after`] with a provenance tag — see the trace module docs.
pub fn request_draw_after_tagged(delay: Duration, reason: &'static str) {
    record_draw_trace(reason);
    request_draw_after(delay);
}

std::thread_local! {
    static NEEDS_DRAW:        Cell<bool>            = Cell::new(false);
    static NEXT_DRAW_AT:      Cell<Option<Instant>> = Cell::new(None);
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
    /// Per-thread snapshot of `ASYNC_WAKEUP_COUNTER` last observed by
    /// [`pump_async_wakeup`].  When the global atomic is ahead of this,
    /// the current thread's [`NEEDS_DRAW`], [`INVALIDATION_EPOCH`] and
    /// [`ASYNC_STATE_EPOCH`] are bumped — see the module docs above
    /// `ASYNC_WAKEUP_COUNTER` for why this indirection is required.
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

/// Process-global counter bumped by [`signal_async_state_change`] from
/// any thread.  The async fetch / decode runs on a background worker
/// (e.g. ehttp's `std::thread::spawn`), so thread-locals it sets are
/// invisible to the main event loop.  The main thread pumps this
/// atomic into its own thread-local epochs on every
/// `wants_draw` / `invalidation_epoch` / `async_state_epoch` read —
/// see [`pump_async_wakeup`].
static ASYNC_WAKEUP_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    let current = ASYNC_WAKEUP_COUNTER.load(Ordering::Acquire);
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
        NEEDS_DRAW.with(|c| c.set(true));
        INVALIDATION_EPOCH.with(|c| c.set(c.get().wrapping_add(1)));
        ASYNC_STATE_EPOCH.with(|c| c.set(c.get().wrapping_add(1)));
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
    NEEDS_DRAW.with(|c| c.set(true));
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
/// come due (`Instant::now() >= deadline`), this clears the scheduled cell and
/// raises [`NEEDS_DRAW`], returning `true`.  That makes a due deadline
/// indistinguishable from an immediate [`request_draw`]: the normal
/// request_draw → paint → [`clear_draw_request`] cycle then applies, and
/// consumers re-arm their next deadline during that paint (so recurring timers
/// stay alive).  This is what lets a purely reactive host serve scheduled
/// draws without relying on a `WaitUntil` surviving intact — see the module
/// docs on the lost-wakeup fix.
pub fn wants_draw() -> bool {
    pump_async_wakeup();
    if NEEDS_DRAW.with(|c| c.get()) {
        return true;
    }
    let due = NEXT_DRAW_AT.with(|c| match c.get() {
        Some(when) if Instant::now() >= when => {
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
/// font loaded, etc.).  Safe to call from any thread; the main event
/// loop observes the bump via [`pump_async_wakeup`] on its next
/// `wants_draw` / `invalidation_epoch` / `async_state_epoch` read.
///
/// This used to only bump thread-local epochs, which silently broke
/// when callers ran on background threads (ehttp spawns its own
/// `std::thread`) — the main thread never observed the change and
/// `render_app_frame`'s layout-key cache skipped the layout pass that
/// would have given freshly-decoded SVG badges their natural
/// dimensions (the user-visible "wrong scale until any other event"
/// bug).
pub fn signal_async_state_change() {
    // Cross-thread visible bump.  Main thread merges via pump_async_wakeup.
    ASYNC_WAKEUP_COUNTER.fetch_add(1, Ordering::AcqRel);
    // Best-effort thread-local bump for same-thread callers (most
    // hosts / tests).  Background threads only set their own
    // thread-locals here, which is harmless — the atomic above is
    // what the main thread actually consumes.
    NEEDS_DRAW.with(|c| c.set(true));
    INVALIDATION_EPOCH.with(|c| c.set(c.get().wrapping_add(1)));
    ASYNC_STATE_EPOCH.with(|c| c.set(c.get().wrapping_add(1)));
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
/// Also syncs this thread's cross-thread async-wakeup bookkeeping so a
/// stale bump from before this clear cannot reappear on the next
/// `wants_draw` read.  Without that sync, parallel tests calling
/// [`signal_async_state_change`] would leak wakeups into unrelated
/// tests that rely on `wants_draw()` returning `false` after a clear.
pub fn clear_draw_request() {
    NEEDS_DRAW.with(|c| c.set(false));
    NEXT_DRAW_AT.with(|c| c.set(None));
    let current = ASYNC_WAKEUP_COUNTER.load(Ordering::Acquire);
    LAST_SEEN_ASYNC_WAKEUP.with(|c| c.set(current));
}

/// Schedule a future draw.  Keeps the EARLIEST pending deadline, so multiple
/// widgets asking for different delays will all be served by the soonest one
/// (each widget re-arms its own deadline on the next draw anyway).
pub fn request_draw_after(delay: Duration) {
    let when = Instant::now() + delay;
    NEXT_DRAW_AT.with(|c| match c.get() {
        Some(existing) if existing <= when => {}
        _ => c.set(Some(when)),
    });
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
            self.start_time = Some(Instant::now());
        }
    }

    /// Advance the animation based on elapsed wall time and return the new
    /// interpolated value.  Ease-out cubic.  While in flight this also calls
    /// [`request_draw`] so the host keeps drawing frames until completion.
    pub fn tick(&mut self) -> f64 {
        if let Some(start) = self.start_time {
            let elapsed = start.elapsed().as_secs_f64();
            let p = (elapsed / self.duration).min(1.0);
            let eased = 1.0 - (1.0 - p).powi(3);
            self.current = self.start_value + (self.target - self.start_value) * eased;
            if p >= 1.0 {
                self.current = self.target;
                self.start_time = None;
            } else {
                request_draw();
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
mod scheduled_draw_tests {
    //! Regression coverage for the reactive-host lost-wakeup fix: the
    //! scheduled-draw cell must be readable non-destructively, and a due
    //! deadline must surface through `wants_draw`. Uses short real sleeps
    //! (`web_time::Instant` has no injectable clock here); each test clears
    //! shared thread-local state up front so it can't inherit a pending
    //! deadline from a prior test on the same worker thread.
    use super::*;
    use std::thread::sleep;

    /// (a) The lost-wakeup repro. A pending deadline read once must still be
    /// visible on the SECOND read — the "intervening AboutToWait" that the
    /// read-and-clear design silently dropped.
    #[test]
    fn peek_is_non_destructive() {
        clear_draw_request();
        request_draw_after(Duration::from_millis(50));
        let first = peek_next_draw_deadline();
        assert!(first.is_some(), "first peek sees the pending deadline");
        let second = peek_next_draw_deadline();
        assert_eq!(
            first, second,
            "second peek still sees the SAME pending deadline (lost-wakeup fix)"
        );
    }

    /// (b) Once due, `wants_draw` returns true; after the paint-clear cycle
    /// consumes it, a subsequent `wants_draw` is false absent a re-arm.
    #[test]
    fn due_deadline_surfaces_then_clears() {
        clear_draw_request();
        assert!(!wants_draw(), "baseline: nothing pending after clear");
        request_draw_after(Duration::from_millis(20));
        sleep(Duration::from_millis(40));
        assert!(wants_draw(), "a due deadline makes wants_draw() true");
        // Simulate the frame that honours it: paint clears the draw flags.
        clear_draw_request();
        assert!(
            !wants_draw(),
            "without a re-arm the loop goes idle again after the draw"
        );
    }

    /// (c) A future (not-yet-due) deadline is peekable but does NOT make
    /// `wants_draw` true — the host idles on `WaitUntil` instead of polling.
    #[test]
    fn future_deadline_peeks_but_does_not_want_draw() {
        clear_draw_request();
        request_draw_after(Duration::from_millis(500));
        assert!(
            peek_next_draw_deadline().is_some(),
            "future deadline is visible to the host's WaitUntil"
        );
        assert!(
            !wants_draw(),
            "a future deadline must not force continuous polling"
        );
        // It also stays pending after that wants_draw() read.
        assert!(
            peek_next_draw_deadline().is_some(),
            "a non-due wants_draw() must not consume the deadline"
        );
    }

    /// (d) Earliest-deadline-wins still holds regardless of arm order.
    #[test]
    fn earliest_deadline_wins() {
        clear_draw_request();
        request_draw_after(Duration::from_millis(400));
        let after_long = peek_next_draw_deadline().expect("long deadline armed");
        request_draw_after(Duration::from_millis(20));
        let after_short = peek_next_draw_deadline().expect("short deadline armed");
        assert!(
            after_short < after_long,
            "a nearer deadline replaces a farther one"
        );
        // Reverse order: a farther deadline does not push the nearer one out.
        request_draw_after(Duration::from_millis(400));
        assert_eq!(
            peek_next_draw_deadline(),
            Some(after_short),
            "arming a farther deadline keeps the earliest"
        );
    }
}
