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
//!    [`take_next_draw_deadline`].  A
//!    widget that needs a draw *at a future time* (text-cursor blink,
//!    tooltip delay) calls `request_draw_after(Duration)`; the host's
//!    loop goes to sleep with `ControlFlow::WaitUntil(that_instant)` and
//!    draws when the deadline fires.  Successive calls keep the EARLIEST
//!    deadline.
//!
//! The host loop draws iff `wants_draw() || now >= take_next_draw_deadline()`.
//! Between draws it idles; no frames are drawn while nothing has changed.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use web_time::Instant;

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

/// Non-destructive read.  Hosts call this after drawing to decide control-flow
/// for the next loop iteration.
///
/// Pumps any pending cross-thread async-wakeup bumps first, so a fetch
/// callback that finished on a worker thread between frames is reflected
/// in the result.
pub fn wants_draw() -> bool {
    pump_async_wakeup();
    NEEDS_DRAW.with(|c| c.get())
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

/// Read-and-clear the scheduled draw deadline.  The host reads this after
/// drawing so the next frame's scheduled wake is determined entirely by what
/// the fresh draw registered (e.g. a text field re-arms the 500 ms blink
/// each frame while it remains focused; losing focus means no re-arm and the
/// loop goes idle).
pub fn take_next_draw_deadline() -> Option<Instant> {
    NEXT_DRAW_AT.with(|c| c.replace(None))
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
