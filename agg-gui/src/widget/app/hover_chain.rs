//! Pointer enter/leave notification along the hovered chain.
//!
//! `MouseMove` goes to the deepest widget under the pointer and stops at the
//! first widget that handles it, so a composite whose child takes the moves
//! (a field wrapper around a `TextField`, a tab strip whose tabs handle
//! hover) never learns the pointer arrived or left. agg-sharp solves this
//! with `MouseEnterBounds` / `MouseLeaveBounds`, raised on every widget in
//! the hovered chain; this module is that mechanism for [`App`].
//!
//! The *hovered chain* is the root plus every widget on the hover path. The
//! *covered* widgets are the others under the pointer: a widget beneath a
//! sibling drawn over it, and that widget's children (agg-sharp visits every
//! child on a move, so each one in bounds is `UnderMouseNotFirst`). Each
//! time the pointer moves, [`App::update_hover_chain`] diffs the new chain
//! and covered set against the ones last announced and sends
//! [`Event::MouseLeave`] to every widget no longer under the pointer
//! (deepest first), then [`Event::MouseEnter`] to every widget newly under
//! it (shallowest first); a widget that moves between the chain and the
//! covered set stays under the pointer and gets neither. Delivery is direct
//! (no bubbling) and happens before the move is dispatched, so who handles
//! `MouseMove` is unchanged.
//!
//! Pointer capture follows agg-sharp's `OnMouseMoveWhenCaptured`: while a
//! widget holds capture, its ancestors (and the covered widgets) stay as
//! they were wherever the pointer goes, the captured widget leaves and
//! re-enters as the pointer crosses its own area (even under a sibling drawn
//! over it), and no other widget is notified. When capture ends, the release
//! re-resolves hover (`App::release` in `pointer.rs`) and the hovered set
//! catches up in one diff.
//!
//! The chain's deepest widget is also the *first under the mouse* (except
//! while capture holds and the pointer is off the captured widget); a widget
//! that gains or loses that gets [`Event::MouseOver`] / [`Event::MouseOut`]
//! (agg-sharp's `MouseEnter` / `MouseLeave`): out before the leaves, over
//! after the enters, matching agg-sharp's per-widget order. The new chain is
//! published (`under_mouse.rs`) before any of these is delivered, so every
//! handler reads the settled state of every widget.
//!
//! A press updates the chain too (agg-sharp's `OnMouseDown` sets
//! `UnderMouseState`), so a press without a preceding move is announced.
//!
//! The announced chain is stored as an anchored path (see `path_anchor.rs`)
//! so it follows reordered children; a widget that was dropped from the tree
//! since it entered gets no leave, and its index is never mistaken for the
//! widget that now sits there.

use super::path_anchor::{anchor_of, identity, resolve};
use crate::event::Event;
use crate::geometry::Point;
use crate::widget::tree::{child_local_pos, deliver_exact};
use crate::widget::{walk_path, App, Widget};

/// The covered part of the hovered set: widgets the pointer is over that are
/// not on the hovered chain (agg-sharp's `UnderMouseNotFirst` widgets under a
/// sibling drawn above them), each with the identities along its path so a
/// reorder is followed and a dropped widget is never mistaken for another.
#[derive(Default)]
pub(crate) struct HoverCovered {
    entries: Vec<(Vec<usize>, Vec<usize>)>,
}

impl HoverCovered {
    /// The covered paths.
    pub(super) fn paths(&self) -> impl Iterator<Item = &[usize]> {
        self.entries.iter().map(|(p, _)| p.as_slice())
    }
}

/// Every widget (by path) whose own area holds `local`: agg-sharp's
/// `OnMouseMoveNotCaptured` visits every child, not just the topmost, and
/// each one under the pointer is under the mouse. The same visibility, hit
/// and exclusivity rules as the hit test (`tree::hit_test_subtree`) apply.
fn in_bounds_paths(
    widget: &dyn Widget,
    local: Point,
    path: &mut Vec<usize>,
    out: &mut Vec<Vec<usize>>,
) {
    if !widget.is_visible() || !widget.hit_test(local) {
        return;
    }
    out.push(path.clone());
    if widget.claims_pointer_exclusively(local) || widget.blocks_child_interaction() {
        return;
    }
    for (i, child) in widget.children().iter().enumerate().rev() {
        let child_local = child_local_pos(widget, child.bounds(), local);
        path.push(i);
        in_bounds_paths(child.as_ref(), child_local, path, out);
        path.pop();
    }
}

/// The identity of the widget at `path`, when it still resolves.
fn identity_at(root: &dyn Widget, path: &[usize]) -> Option<usize> {
    walk_path(root, path).map(identity)
}

/// Whether root-space point `at` lies in the own area (`hit_test`) of the
/// widget at `path`, whatever is drawn over it; `None` when the path no
/// longer resolves.
fn point_in_widget(root: &dyn Widget, path: &[usize], at: Point) -> Option<bool> {
    let mut node = root;
    let mut local = at;
    for &idx in path {
        let child = node.children().get(idx)?;
        local = child_local_pos(node, child.bounds(), local);
        node = child.as_ref();
    }
    Some(node.hit_test(local))
}

/// The chain to announce for hit path `hit` given pointer capture `captured`:
/// the hit path itself without capture; with capture, the captured path while
/// the pointer is over the captured widget and the captured widget's
/// ancestors otherwise. `over_area` says whether the pointer is in the
/// captured widget's own area (agg-sharp's `PositionWithinLocalBounds` in
/// `OnMouseMoveWhenCaptured`, so a sibling drawn over it does not make it
/// leave); without it (a modal's hit) the hit path decides.
fn target_chain(
    hit: Option<&[usize]>,
    captured: Option<&[usize]>,
    over_area: Option<bool>,
) -> Option<Vec<usize>> {
    let Some(cap) = captured else {
        return hit.map(<[usize]>::to_vec);
    };
    let over_captured = over_area.unwrap_or_else(|| hit.is_some_and(|h| h.starts_with(cap)));
    match cap.split_last() {
        _ if over_captured => Some(cap.to_vec()),
        Some((_, ancestors)) => Some(ancestors.to_vec()),
        // The root holds capture and the pointer is off it (out of the
        // window): the root itself leaves.
        None => None,
    }
}

impl App {
    /// The covered widgets for a pointer at `at` (root space) whose hovered
    /// chain is `chain`: every widget under the pointer that is not on the
    /// chain. While capture holds they stay as they were (agg-sharp only
    /// updates the capture path then); with no position (a modal's hit, the
    /// pointer gone) there are none.
    fn covered_for(&mut self, chain: Option<&[usize]>, at: Option<Point>) -> HoverCovered {
        if self.captured.is_some() {
            return std::mem::take(&mut self.hover_covered);
        }
        let (Some(at), Some(chain)) = (at, chain) else {
            return HoverCovered::default();
        };
        let mut all = Vec::new();
        in_bounds_paths(self.root.as_ref(), at, &mut Vec::new(), &mut all);
        let root = self.root.as_ref();
        HoverCovered {
            entries: all
                .into_iter()
                .filter(|p| !chain.starts_with(p))
                .map(|p| {
                    let anchor = anchor_of(root, Some(&p));
                    (p, anchor)
                })
                .collect(),
        }
    }

    /// Announce the hovered chain for hit path `hit` (already resolved by the
    /// caller), sending `MouseLeave` / `MouseEnter` to every widget whose
    /// membership changed. Call before dispatching the `MouseMove`. `at` is
    /// the pointer in root space, from which the widgets it covers are found
    /// (`None` for a modal's hit, which covers nothing).
    pub(super) fn update_hover_chain(&mut self, hit: Option<&[usize]>, at: Option<Point>) {
        let over_area = match (self.captured.as_deref(), at) {
            (Some(cap), Some(at)) => point_in_widget(self.root.as_ref(), cap, at),
            _ => None,
        };
        let new = target_chain(hit, self.captured.as_deref(), over_area);
        let old = self.hover_chain.take();

        // The covered sets, before and after, each entry resolved to where
        // its widget sits now; an entry whose widget is gone is dropped.
        let mut old_covered = std::mem::take(&mut self.hover_covered);
        {
            let root = self.root.as_ref();
            old_covered.entries.retain_mut(|(path, anchor)| {
                resolve(root, path, anchor);
                anchor_of(root, Some(path)) == *anchor
            });
        }
        self.hover_covered = old_covered;
        let new_covered = self.covered_for(new.as_deref(), at);
        let old_covered = std::mem::replace(&mut self.hover_covered, new_covered);

        // How deep the old chain still names the widgets it entered: after
        // `resolve_tracked_paths`, a level whose widget was dropped keeps its
        // stale index, so compare identities level by level.
        let old_valid = old.as_ref().map_or(0, |old| {
            let now = anchor_of(self.root.as_ref(), Some(old));
            now.iter()
                .zip(&self.anchors.hover_chain)
                .take_while(|(a, b)| a == b)
                .count()
        });

        // Number of chain levels (root = level 0, so a path of length n has
        // n + 1 levels) shared by old and new.
        let shared = match (&old, &new) {
            (Some(old), Some(new)) => {
                let common = old.iter().zip(new).take_while(|(a, b)| a == b).count();
                common.min(old_valid) + 1
            }
            _ => 0,
        };

        // Every widget under the mouse before and after (chain and covered),
        // by identity: one that moves between the chain and the covered set
        // stays under the mouse and gets neither a leave nor an enter.
        let (old_ids, new_ids) = {
            let root = self.root.as_ref();
            let chain_ids = |chain: &Option<Vec<usize>>, valid: usize| -> Vec<usize> {
                match chain {
                    Some(c) => (0..=c.len().min(valid))
                        .filter_map(|level| identity_at(root, &c[..level]))
                        .collect(),
                    None => Vec::new(),
                }
            };
            let mut old_ids = chain_ids(&old, old_valid);
            old_ids.extend(
                old_covered
                    .entries
                    .iter()
                    .filter_map(|(_, a)| a.last().copied()),
            );
            let mut new_ids = chain_ids(&new, usize::MAX);
            new_ids.extend(
                self.hover_covered
                    .entries
                    .iter()
                    .filter_map(|(_, a)| a.last().copied()),
            );
            (old_ids, new_ids)
        };

        // The first-under-mouse widget is the chain's deepest unless capture
        // holds and the pointer is off the captured widget.
        let new_first = new.is_some() && (self.captured.is_none() || self.captured == new);
        let old_first = std::mem::replace(&mut self.hover_first, new_first);
        // The old first widget stays first only when the whole old chain is
        // kept and is the whole new chain.
        let first_kept = old_first
            && new_first
            && matches!((&old, &new), (Some(o), Some(n)) if o.len() == n.len() && shared == o.len() + 1);
        self.publish_under_mouse(new.as_deref(), new_first, &self.hover_covered);

        let stays = |root: &dyn Widget, path: &[usize], ids: &[usize]| {
            identity_at(root, path).is_some_and(|id| ids.contains(&id))
        };
        if let Some(old) = &old {
            // The old first widget loses that first (agg-sharp raises
            // `MouseLeave` before `MouseLeaveBounds`), if it is still the
            // widget that entered.
            if old_first && !first_kept && old_valid >= old.len() {
                deliver_exact(self.root.as_mut(), old, &Event::MouseOut);
            }
            // Levels `shared..=old.len()` leave, deepest first; only levels
            // whose widget is still the one that entered are reachable.
            let deepest = old.len().min(old_valid);
            for level in (shared..=deepest).rev() {
                if !stays(self.root.as_ref(), &old[..level], &new_ids) {
                    deliver_exact(self.root.as_mut(), &old[..level], &Event::MouseLeave);
                }
            }
        }
        for (path, anchor) in old_covered.entries.iter().rev() {
            if anchor.last().is_some_and(|id| !new_ids.contains(id)) {
                deliver_exact(self.root.as_mut(), path, &Event::MouseLeave);
            }
        }
        if let Some(new) = &new {
            for level in shared..=new.len() {
                if !stays(self.root.as_ref(), &new[..level], &old_ids) {
                    deliver_exact(self.root.as_mut(), &new[..level], &Event::MouseEnter);
                }
            }
        }
        let entering: Vec<Vec<usize>> = self
            .hover_covered
            .entries
            .iter()
            .filter(|(_, a)| a.last().is_some_and(|id| !old_ids.contains(id)))
            .map(|(p, _)| p.clone())
            .collect();
        for path in entering {
            deliver_exact(self.root.as_mut(), &path, &Event::MouseEnter);
        }
        if let Some(new) = &new {
            if new_first && !first_kept {
                deliver_exact(self.root.as_mut(), new, &Event::MouseOver);
            }
        }

        self.anchors.hover_chain = anchor_of(self.root.as_ref(), new.as_deref());
        self.hover_chain = new;
    }
}

#[cfg(test)]
mod tests {
    use super::target_chain;

    #[test]
    fn target_without_capture_is_the_hit() {
        assert_eq!(target_chain(Some(&[0, 1]), None, None), Some(vec![0, 1]));
        assert_eq!(target_chain(None, None, None), None);
    }

    #[test]
    fn target_with_capture_keeps_ancestors_and_toggles_the_captured_widget() {
        let cap: &[usize] = &[0, 1];
        assert_eq!(
            target_chain(Some(&[0, 1]), Some(cap), None),
            Some(vec![0, 1])
        );
        // Inside the captured widget: the chain stops at it.
        assert_eq!(
            target_chain(Some(&[0, 1, 2]), Some(cap), None),
            Some(vec![0, 1])
        );
        // Over a sibling or out of the window: only the ancestors remain.
        assert_eq!(target_chain(Some(&[0, 2]), Some(cap), None), Some(vec![0]));
        assert_eq!(target_chain(None, Some(cap), None), Some(vec![0]));
        // Root capture off the window: nothing is hovered.
        assert_eq!(target_chain(None, Some(&[]), None), None);
        // The captured widget's own area decides when it is known: a sibling
        // drawn over it does not make it leave, and the pointer off its area
        // leaves even when the hit says otherwise.
        assert_eq!(
            target_chain(Some(&[0, 2]), Some(cap), Some(true)),
            Some(vec![0, 1])
        );
        assert_eq!(
            target_chain(Some(&[0, 1]), Some(cap), Some(false)),
            Some(vec![0])
        );
    }
}
