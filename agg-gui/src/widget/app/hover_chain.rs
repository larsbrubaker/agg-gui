//! Pointer enter/leave notification along the hovered chain.
//!
//! `MouseMove` goes to the deepest widget under the pointer and stops at the
//! first widget that handles it, so a composite whose child takes the moves
//! (a field wrapper around a `TextField`, a tab strip whose tabs handle
//! hover) never learns the pointer arrived or left. agg-sharp solves this
//! with `MouseEnterBounds` / `MouseLeaveBounds`, raised on every widget in
//! the hovered chain; this module is that mechanism for [`App`].
//!
//! The *hovered chain* is the root plus every widget on the hover path. Each
//! time the pointer moves, [`App::update_hover_chain`] diffs the new chain
//! against the one last announced and sends [`Event::MouseLeave`] to every
//! widget that dropped off (deepest first), then [`Event::MouseEnter`] to
//! every widget that joined (shallowest first). Delivery is direct (no
//! bubbling) and happens before the move is dispatched, so who handles
//! `MouseMove` is unchanged.
//!
//! Pointer capture follows agg-sharp's `OnMouseMoveWhenCaptured`: while a
//! widget holds capture, its ancestors stay on the chain wherever the pointer
//! goes, the captured widget leaves and re-enters as the pointer crosses its
//! own area, and no other widget is notified. When capture ends, the release
//! re-resolves hover (`App::refresh_hover_after_release`) and the chain
//! catches up in one diff.
//!
//! The announced chain is stored as an anchored path (see `path_anchor.rs`)
//! so it follows reordered children; a widget that was dropped from the tree
//! since it entered gets no leave, and its index is never mistaken for the
//! widget that now sits there.

use super::path_anchor::anchor_of;
use crate::event::Event;
use crate::widget::tree::deliver_exact;
use crate::widget::App;

/// The chain to announce for hit path `hit` given pointer capture `captured`:
/// the hit path itself without capture; with capture, the captured path while
/// the pointer is over the captured widget (or inside it) and the captured
/// widget's ancestors otherwise.
fn target_chain(hit: Option<&[usize]>, captured: Option<&[usize]>) -> Option<Vec<usize>> {
    let Some(cap) = captured else {
        return hit.map(<[usize]>::to_vec);
    };
    let over_captured = hit.is_some_and(|h| h.starts_with(cap));
    match cap.split_last() {
        _ if over_captured => Some(cap.to_vec()),
        Some((_, ancestors)) => Some(ancestors.to_vec()),
        // The root holds capture and the pointer is off it (out of the
        // window): the root itself leaves.
        None => None,
    }
}

impl App {
    /// Announce the hovered chain for hit path `hit` (already resolved by the
    /// caller), sending `MouseLeave` / `MouseEnter` to every widget whose
    /// membership changed. Call before dispatching the `MouseMove`.
    pub(super) fn update_hover_chain(&mut self, hit: Option<&[usize]>) {
        let new = target_chain(hit, self.captured.as_deref());
        let old = self.hover_chain.take();

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

        if let Some(old) = &old {
            // Levels `shared..=old.len()` leave, deepest first; only levels
            // whose widget is still the one that entered are reachable.
            let deepest = old.len().min(old_valid);
            for level in (shared..=deepest).rev() {
                deliver_exact(self.root.as_mut(), &old[..level], &Event::MouseLeave);
            }
        }
        if let Some(new) = &new {
            for level in shared..=new.len() {
                deliver_exact(self.root.as_mut(), &new[..level], &Event::MouseEnter);
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
        assert_eq!(target_chain(Some(&[0, 1]), None), Some(vec![0, 1]));
        assert_eq!(target_chain(None, None), None);
    }

    #[test]
    fn target_with_capture_keeps_ancestors_and_toggles_the_captured_widget() {
        let cap: &[usize] = &[0, 1];
        assert_eq!(target_chain(Some(&[0, 1]), Some(cap)), Some(vec![0, 1]));
        // Inside the captured widget: the chain stops at it.
        assert_eq!(target_chain(Some(&[0, 1, 2]), Some(cap)), Some(vec![0, 1]));
        // Over a sibling or out of the window: only the ancestors remain.
        assert_eq!(target_chain(Some(&[0, 2]), Some(cap)), Some(vec![0]));
        assert_eq!(target_chain(None, Some(cap)), Some(vec![0]));
        // Root capture off the window: nothing is hovered.
        assert_eq!(target_chain(None, Some(&[])), None);
    }
}
