//! Identity anchors that keep [`App`]'s stored widget paths pointing at the
//! same widgets when a parent reorders its children.
//!
//! `App` addresses the focused, hovered, pointer-captured and
//! gesture-captured widgets by child-index path (`Vec<usize>`). A path alone
//! goes stale the moment a parent reorders its children — a tab strip moving
//! the dragged tab past a neighbour, a list re-sorting — and the rest of the
//! drag (moves *and* the release) would land on whichever widget now sits at
//! the old index. So every time one of those paths is assigned, this module
//! also records an *anchor*: the identity of each widget along the path. Before
//! the paths are used again ([`App::resolve_tracked_paths`], run after every
//! layout pass and at the start of each input entry point) each path is
//! re-resolved against its anchor, following its widgets to their new indices.
//!
//! A widget's identity is the address of its heap allocation: children live in
//! `Vec<Box<dyn Widget>>`, and reordering the vector moves the boxes, never the
//! widgets they point to. No widget opts in and no API changes — any reorder
//! of an existing child (swap, remove + insert, rebuild of the `Vec` from the
//! same boxes) is followed. A widget that was dropped has no new index; its
//! path is left as it was (dispatch already tolerates stale paths, see
//! `tree::dispatch_event`). The current index is checked first, so a path whose
//! widgets did not move resolves in O(depth) without searching.
//!
//! `maybe_bring_to_front` (in `app.rs`) still shifts the paths itself when it
//! raises a window; the anchors stay valid across that because identities do
//! not change.

use crate::widget::{App, Widget};

/// The identity of each widget along one stored path (one entry per path
/// element). Empty when the slot holds no path.
type Anchor = Vec<usize>;

/// Anchors for the four index paths [`App`] keeps between events.
#[derive(Default)]
pub(super) struct TrackedAnchors {
    focus: Anchor,
    hovered: Anchor,
    captured: Anchor,
    gesture_captured: Anchor,
}

/// The identity of a widget: the address of its data. Stable while the widget
/// lives, whatever happens to the `Box` that owns it.
fn identity(w: &dyn Widget) -> usize {
    w as *const dyn Widget as *const () as usize
}

/// Record the identity of every widget along `path`. Stops at the first index
/// that does not exist (the anchor then covers the valid prefix).
fn anchor_of(root: &dyn Widget, path: Option<&[usize]>) -> Anchor {
    let mut anchor = Vec::new();
    let Some(path) = path else {
        return anchor;
    };
    let mut node = root;
    for &idx in path {
        let Some(child) = node.children().get(idx) else {
            break;
        };
        node = child.as_ref();
        anchor.push(identity(node));
    }
    anchor
}

/// Re-point `path` at the widgets recorded in `anchor`, level by level. At
/// each level the current index is kept when it still holds the anchored
/// widget; otherwise the parent's children are searched for it. Resolution
/// stops (leaving the rest of the path untouched) at a level whose widget is
/// gone.
fn resolve(root: &dyn Widget, path: &mut [usize], anchor: &[usize]) {
    let mut node = root;
    for (depth, &want) in anchor.iter().enumerate() {
        let Some(slot) = path.get_mut(depth) else {
            return;
        };
        let children = node.children();
        let at_index = children
            .get(*slot)
            .is_some_and(|c| identity(c.as_ref()) == want);
        if !at_index {
            match children.iter().position(|c| identity(c.as_ref()) == want) {
                Some(found) => *slot = found,
                None => return,
            }
        }
        node = children[*slot].as_ref();
    }
}

impl App {
    /// Follow every stored path (focus, hover, pointer capture, gesture
    /// capture) to where its widgets now sit, after any reordering of
    /// children since the path was recorded.
    pub(super) fn resolve_tracked_paths(&mut self) {
        let root = self.root.as_ref();
        let anchors = &self.anchors;
        for (path, anchor) in [
            (&mut self.focus, &anchors.focus),
            (&mut self.hovered, &anchors.hovered),
            (&mut self.captured, &anchors.captured),
            (&mut self.gesture_captured, &anchors.gesture_captured),
        ] {
            if let Some(path) = path {
                resolve(root, path, anchor);
            }
        }
    }

    /// Store the focus path and anchor it to the widgets it names now.
    pub(super) fn store_focus(&mut self, path: Option<Vec<usize>>) {
        self.anchors.focus = anchor_of(self.root.as_ref(), path.as_deref());
        self.focus = path;
    }

    /// Store the hover path and anchor it to the widgets it names now.
    pub(super) fn store_hovered(&mut self, path: Option<Vec<usize>>) {
        self.anchors.hovered = anchor_of(self.root.as_ref(), path.as_deref());
        self.hovered = path;
    }

    /// Store the pointer-capture path and anchor it to the widgets it names
    /// now.
    pub(super) fn store_captured(&mut self, path: Option<Vec<usize>>) {
        self.anchors.captured = anchor_of(self.root.as_ref(), path.as_deref());
        self.captured = path;
    }

    /// Store the gesture-capture path and anchor it to the widgets it names
    /// now.
    pub(super) fn store_gesture_captured(&mut self, path: Option<Vec<usize>>) {
        self.anchors.gesture_captured = anchor_of(self.root.as_ref(), path.as_deref());
        self.gesture_captured = path;
    }
}
