//! `PointerReach` — the port of agg-sharp `GuiAutomation/PointerReach.cs`:
//! whether a press aimed at a widget can get to it, as the mouse routes one.
//!
//! The pointer router (`agg_gui::widget::hit_test_subtree`) asks every
//! ancestor's [`Widget::hit_test`] before the press goes into its children,
//! so a widget can be drawn yet out of the pointer's reach — the GUI demo's
//! closing window stays on the canvas while it fades out but lets every press
//! through to what is under it. When several widgets share a name (each demo
//! window has its own "Window Collapse Button") a click aimed at such a one
//! lands on whatever is beneath and does nothing, and how long the fade
//! lasted in frames decided whether a test met it. The runner's
//! `get_widget_by_name` therefore prefers widgets a press can reach.

use agg_gui::{Point, Widget};

use crate::tree_query::{NamedHit, WidgetHandle};

/// True when no ancestor of `handle`'s widget refuses a press at its centre
/// (C# `CanReach`). The widget itself is not asked: a shaped control may
/// answer false at its centre and still be clicked where it is drawn. False
/// once the handle is detached.
pub fn can_reach(root: &dyn Widget, handle: &WidgetHandle) -> bool {
    let Some(path) = handle.resolve(root) else {
        return false;
    };
    // The chain root → widget, so the walk back up can see each parent.
    let mut chain: Vec<&dyn Widget> = vec![root];
    for &idx in &path {
        let Some(child) = chain[chain.len() - 1].children().get(idx) else {
            return false;
        };
        chain.push(child.as_ref());
    }

    let target = chain[chain.len() - 1].bounds();
    let mut centre = Point::new(target.width / 2.0, target.height / 2.0);
    for depth in (0..path.len()).rev() {
        let (parent, child) = (chain[depth], chain[depth + 1]);
        // Child-local → parent-local, the inverse of the router's mapping:
        // add the child's offset, then apply the parent's child transform.
        let offset = child.bounds();
        let (mut x, mut y) = (centre.x + offset.x, centre.y + offset.y);
        if let Some(t) = parent.child_transform() {
            t.transform(&mut x, &mut y);
        }
        centre = Point::new(x, y);
        if !parent.hit_test(centre) {
            return false;
        }
    }
    true
}

/// The hits a click should choose among: those the pointer can reach, or all
/// of them when it can reach none (the click then goes where it always did,
/// rather than the lookup failing). C# `PreferReachable`.
pub fn prefer_reachable(root: &dyn Widget, results: Vec<NamedHit>) -> Vec<NamedHit> {
    let reachable: Vec<NamedHit> = results
        .iter()
        .filter(|hit| can_reach(root, &hit.handle))
        .cloned()
        .collect();
    if reachable.is_empty() {
        results
    } else {
        reachable
    }
}
