//! Identity anchors that keep [`App`]'s stored widget paths pointing at the
//! same widgets when a parent reorders its children.
//!
//! `App` addresses the focused, hovered, pointer-captured and
//! gesture-captured widgets (and the enter/leave hover chain) by child-index path (`Vec<usize>`). A path alone
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
//! A path whose widget was *replaced* by a same-shaped one resolves to the
//! replacement, which never received `FocusGained`;
//! `App::focus_is_anchored` detects that so `App::set_focus` can treat
//! focusing the replacement as a real focus change.
//!
//! [`WidgetAnchor`] exposes the same anchoring for code outside `App` (GUI
//! automation's widget handles).
//!
//! `maybe_bring_to_front` (in `app.rs`) still shifts the paths itself when it
//! raises a window; the anchors stay valid across that because identities do
//! not change.

use crate::widget::{App, Widget};

/// The identity of each widget along one stored path (one entry per path
/// element). Empty when the slot holds no path.
type Anchor = Vec<usize>;

/// Anchors for the index paths [`App`] keeps between events.
#[derive(Default)]
pub(super) struct TrackedAnchors {
    focus: Anchor,
    hovered: Anchor,
    captured: Anchor,
    gesture_captured: Anchor,
    /// The hovered chain the last enter/leave notification described (see
    /// `hover_chain.rs`); differs from `hovered` while pointer capture holds.
    pub(super) hover_chain: Anchor,
}

/// The identity of a widget: the address of its data. Stable while the widget
/// lives, whatever happens to the `Box` that owns it.
pub(super) fn identity(w: &dyn Widget) -> usize {
    w as *const dyn Widget as *const () as usize
}

/// Record the identity of every widget along `path`. Stops at the first index
/// that does not exist (the anchor then covers the valid prefix).
pub(super) fn anchor_of(root: &dyn Widget, path: Option<&[usize]>) -> Anchor {
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
pub(super) fn resolve(root: &dyn Widget, path: &mut [usize], anchor: &[usize]) {
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
    /// capture, the enter/leave hover chain) to where its widgets now sit, after any reordering of
    /// children since the path was recorded.
    pub(super) fn resolve_tracked_paths(&mut self) {
        let root = self.root.as_ref();
        let anchors = &self.anchors;
        for (path, anchor) in [
            (&mut self.focus, &anchors.focus),
            (&mut self.hovered, &anchors.hovered),
            (&mut self.captured, &anchors.captured),
            (&mut self.gesture_captured, &anchors.gesture_captured),
            (&mut self.hover_chain, &anchors.hover_chain),
        ] {
            if let Some(path) = path {
                resolve(root, path, anchor);
            }
        }
    }

    /// Whether the stored focus path still names the widgets that were
    /// focused, i.e. the focused widget was not replaced. When an app swaps
    /// the focused subtree for a freshly built one of the same shape, the
    /// path resolves to the new widget, which never received `FocusGained`;
    /// `App::set_focus` uses this to focus it properly on the next click and
    /// to skip `FocusLost` for a widget that never had focus. Call after
    /// [`resolve_tracked_paths`](Self::resolve_tracked_paths).
    pub(super) fn focus_is_anchored(&self) -> bool {
        self.focus
            .as_deref()
            .is_some_and(|p| anchor_of(self.root.as_ref(), Some(p)) == self.anchors.focus)
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

    /// Whether the pointer-capture path still names the widgets it was
    /// anchored to (false with no capture, or once the holder was replaced
    /// or dropped). Call after `resolve_tracked_paths`.
    pub(super) fn capture_holder_present(&self) -> bool {
        self.captured
            .as_deref()
            .is_some_and(|path| anchor_of(self.root.as_ref(), Some(path)) == self.anchors.captured)
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

/// A widget's place in a tree that follows the widget, not its indices: the
/// child-index path to it plus the identity of each widget along that path
/// (the same anchoring [`App`] uses for its focus, hover and capture paths).
///
/// GUI automation holds one per found widget (agg-gui-automation's
/// `WidgetHandle`) across frames: [`resolve`](Self::resolve) follows the
/// widget when a parent reorders its children, and reports it gone once it
/// (or any ancestor) has left the tree. Identity is the widget's heap
/// address, so a widget dropped and replaced by a new allocation at the same
/// address between two resolves reads as still present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetAnchor {
    path: Vec<usize>,
    identities: Vec<usize>,
}

impl WidgetAnchor {
    /// Anchor the widget at `path` under `root` (an empty path is `root`
    /// itself). `None` when the path does not name a widget.
    pub fn new(root: &dyn Widget, path: &[usize]) -> Option<Self> {
        let identities = anchor_of(root, Some(path));
        (identities.len() == path.len()).then(|| Self {
            path: path.to_vec(),
            identities,
        })
    }

    /// The path recorded when the anchor was made (or last re-anchored).
    pub fn path(&self) -> &[usize] {
        &self.path
    }

    /// The widget's current path under `root`, following any reordering of
    /// its ancestors' children; `None` when it or an ancestor is gone.
    pub fn resolve(&self, root: &dyn Widget) -> Option<Vec<usize>> {
        let mut path = self.path.clone();
        resolve(root, &mut path, &self.identities);
        (anchor_of(root, Some(&path)) == self.identities).then_some(path)
    }

    /// [`resolve`](Self::resolve), also storing the resolved path so the next
    /// resolve starts from it.
    pub fn refresh(&mut self, root: &dyn Widget) -> Option<&[usize]> {
        let path = self.resolve(root)?;
        self.path = path;
        Some(&self.path)
    }
}
