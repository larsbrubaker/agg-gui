//! Thread-local registry of currently-laid-out [`Snappable`] rects.
//!
//! Drag handlers can't walk the widget tree to find sibling
//! Snappables (events arrive at one widget, not at the root), so
//! every Snappable participant pushes its current rect into this
//! registry from its `layout()` pass.  The drag handler then snapshots
//! the registry at event time, filters out the dragger by id, and
//! passes the result to [`compute_snap`].
//!
//! The registry is **id-keyed** — re-registering an existing id
//! updates the entry rather than appending — so the list stays bounded
//! at one entry per logical widget across the lifetime of the program.
//! Widgets that go hidden should call [`unregister_target`] so they
//! don't pull other widgets toward stale off-screen bounds.
//!
//! A second thread-local stores the latest [`SnapGuide`] list for the
//! overlay widget to paint.  Drag handlers write guides on every
//! `MouseMove` while dragging and clear them on `MouseUp`.
//!
//! [`Snappable`]: super::Snappable
//! [`compute_snap`]: super::compute_snap

use std::cell::RefCell;

use crate::geometry::Rect;

use super::{SnapGuide, SnapId};

thread_local! {
    static TARGETS: RefCell<Vec<(SnapId, Rect)>> = const { RefCell::new(Vec::new()) };
    static GUIDES: RefCell<Vec<SnapGuide>> = const { RefCell::new(Vec::new()) };
}

/// Add or update this id's entry.  Cheap O(n) over the registry —
/// `n` is the number of active snappables on screen, typically
/// single digits.
pub fn register_target(id: SnapId, rect: Rect) {
    TARGETS.with(|c| {
        let mut t = c.borrow_mut();
        if let Some(slot) = t.iter_mut().find(|(eid, _)| *eid == id) {
            slot.1 = rect;
        } else {
            t.push((id, rect));
        }
    });
}

/// Remove an id's entry.  Call when a Snappable is no longer a
/// legitimate snap target — typically when a window goes hidden or
/// a node is deleted.
pub fn unregister_target(id: SnapId) {
    TARGETS.with(|c| c.borrow_mut().retain(|(eid, _)| *eid != id));
}

/// Snapshot the registry as a `Vec` for passing into
/// [`compute_snap`].
///
/// [`compute_snap`]: super::compute_snap
pub fn targets_snapshot() -> Vec<(SnapId, Rect)> {
    TARGETS.with(|c| c.borrow().clone())
}

/// Test-only escape hatch — drop every entry.  Used by tests that
/// want to start with an empty registry so they don't inherit state
/// from a sibling test that registered something earlier.
#[cfg(test)]
#[allow(dead_code)]
pub fn clear_targets_for_testing() {
    TARGETS.with(|c| c.borrow_mut().clear());
}

/// Replace the guide list the [`SnapOverlay`] should paint on the
/// next frame.  Drag handlers call this after every successful
/// [`compute_snap`] so the guides track the cursor in real time.
///
/// [`SnapOverlay`]: super::overlay::SnapOverlay
/// [`compute_snap`]: super::compute_snap
pub fn set_guides(guides: Vec<SnapGuide>) {
    GUIDES.with(|c| *c.borrow_mut() = guides);
}

/// Snapshot the current guides for the overlay to paint.
pub fn guides_snapshot() -> Vec<SnapGuide> {
    GUIDES.with(|c| c.borrow().clone())
}

/// Drop all guides.  Drag handlers call this on `MouseUp` so the
/// overlay clears when the drag ends.
pub fn clear_guides() {
    GUIDES.with(|c| c.borrow_mut().clear());
}
