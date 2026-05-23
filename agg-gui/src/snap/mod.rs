//! Generic snap-layout system for movable + resizable rects.
//!
//! Reusable across `agg-gui`'s `Window`, AdamArtist's node graph,
//! and any third-party widget whose primary state is a rect that
//! drags and resizes.  The engine is pure — it knows nothing about
//! widgets, events, or paint — so it can be wired into any drag
//! handler that produces a candidate rect and wants a snapped result.
//!
//! ## Pattern
//!
//! 1. Implement [`Snappable`] on your movable type — three accessors
//!    (`snap_id`, `snap_rect`, `set_snap_rect`) and two opt-in flags.
//! 2. When the user drags, collect `(SnapId, Rect)` tuples for every
//!    *other* visible Snappable in the scene (skip the dragger).
//! 3. Call [`compute_snap`] with the dragger's candidate rect, the
//!    target list, a pixel threshold, and a [`SnapMode`].
//! 4. Apply the returned [`SnapResult::rect`] back through
//!    `set_snap_rect`; render the returned [`SnapGuide`]s as overlay
//!    lines.
//!
//! ## Global enable flag
//!
//! Snapping is gated behind a thread-local flag managed via
//! [`is_enabled`] and [`set_enabled`].  Drag handlers should check
//! `is_enabled()` first and skip the engine entirely when off — keeps
//! the gate at the call site so individual widgets don't pay any cost
//! when snapping is disabled.

mod engine;
mod model;
mod overlay;
mod registry;

#[cfg(test)]
mod tests;

pub use engine::compute_snap;
pub use model::{ResizeEdge, SnapGuide, SnapId, SnapMode, SnapResult, Snappable};
pub use overlay::SnapOverlay;
pub use registry::{
    clear_guides, guides_snapshot, register_target, set_guides, targets_snapshot, unregister_target,
};

/// Default pixel distance at which an alignment / spacing match
/// engages.  Apps can pass a different value to [`compute_snap`]
/// directly; this is the value drag handlers should reach for when
/// they have no specific reason to override it.
pub const DEFAULT_THRESHOLD: f64 = 8.0;

/// Mint a fresh [`SnapId`] from a process-wide atomic counter.
/// Cheap — single relaxed increment — so widgets can call it from
/// their constructor without caring about contention.
pub fn next_snap_id() -> SnapId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    SnapId(COUNTER.fetch_add(1, Ordering::Relaxed))
}

use std::cell::Cell;

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
}

/// `true` if snapping should run during drag/resize operations.
/// Drag handlers must check this and skip the snap path entirely
/// when off.
pub fn is_enabled() -> bool {
    ENABLED.with(|c| c.get())
}

/// Toggle the global snap-enable flag.  Persists for the lifetime of
/// the thread — typically wired to a UI checkbox (see the demo's
/// `View > Window Snapping` menu item) and to saved-state
/// persistence so the user's preference survives a relaunch.
pub fn set_enabled(on: bool) {
    ENABLED.with(|c| c.set(on));
}
