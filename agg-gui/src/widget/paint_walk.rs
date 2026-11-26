//! Widgets in the order painting paints them, each where painting puts it:
//! [`walk_painted`] visits the tree as `widget/paint.rs` paints it and hands
//! each widget its [`PaintPlacement`] from `widget/paint_geometry.rs`, so a
//! host can reason about what covers what on screen without a draw context.
//! [`painted_over`] answers from it what paints on top of one widget, the
//! question a floating card asks before it settles over a view (the clear
//! spot itself is `widget/clear_spot.rs`).
//!
//! The order replayed, each rule as painting applies it:
//! - a widget paints before its children, and its children paint in list
//!   order, except that children reporting `draw_on_top_of_siblings` are held
//!   back and paint after the rest, in list order;
//! - a widget that is hidden or `suppresses_paint` paints nothing, children
//!   included;
//! - a subtree that paints in the overlay pass (`defer_paint_to_overlay`)
//!   paints nothing in place: it paints after the whole tree, clipped by the
//!   window alone, in the overlay pass's post-order (a widget's descendants'
//!   overlay subtrees before its own), as `paint_global_overlays` reaches it.
//!
//! The placements, clips and window are the ones
//! [`find_widget_visible_rect`](super::find_widget_visible_rect) gives, so a
//! widget's visible rect from the walk and from that lookup agree.

use super::paint_geometry::{
    children_clip_rect, children_transform, entered, intersect, offset_child, root_transform,
    to_whole_pixels, PaintPlacement,
};
use super::Widget;
use crate::geometry::{Rect, Size};
use crate::TransAffine;
use std::ops::ControlFlow;

/// What [`walk_painted`] does after visiting a widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintWalk {
    /// Go on into the widget's children.
    Descend,
    /// Leave out what the widget's subtree paints in place. A descendant
    /// painted in the overlay pass is still visited there: it paints over
    /// everything, wherever its parent is.
    Skip,
    /// End the walk: nothing more is visited.
    Stop,
}

/// Visit every painted widget under `root` (the root included) in paint
/// order, each with where painting puts it, until `visit` returns
/// [`PaintWalk::Stop`] (see the module docs for the order).
///
/// Hidden subtrees and those under a widget that `suppresses_paint` are never
/// visited, so every placement's `visible_chain` is true. The window is the
/// one [`find_widget_visible_rect`](super::find_widget_visible_rect) uses:
/// the root's bounds, less what the on-screen keyboard's lift pushes off
/// their top. A visited widget may show nothing (scrolled out of view):
/// [`PaintPlacement::visible_rect`] says what of it shows. A child that skips
/// its own paint when clipped out (`skip_paint_when_clipped_out`) is visited
/// all the same; what of it shows is then at most a sub-pixel sliver.
pub fn walk_painted(
    root: &dyn Widget,
    mut visit: impl FnMut(&dyn Widget, &PaintPlacement) -> PaintWalk,
) {
    walk(root, &mut |widget, step| visit(widget, &step.placement));
}

/// The rectangles of what painting paints after the widget carrying
/// `target_id` and that overlaps the part of it that shows: siblings and
/// panels painted later, layers floating over it, and its own descendants
/// (controls placed on a view), in root logical space (the space of
/// [`find_widget_visible_rect`](super::find_widget_visible_rect)), in paint
/// order.
///
/// Every widget painted after the target that overlaps it is listed whole
/// (its visible rect) and the walk does not go into it, so a panel lists as
/// one rectangle, not as each control in it, and an opaque page over the
/// target lists as the page. The exceptions are the widgets `is_layer`
/// accepts, which the host knows paint nothing of their own (a full-window
/// overlay holding a few buttons): the walk goes into such a layer and lists
/// what is in it instead. The target itself is always gone into, as its
/// descendants paint over it. A widget that misses the target is gone into
/// only when its children can paint over it (they are clipped to a rect wider
/// than its own).
///
/// A listed widget whose children are clipped to a rect wider than its own
/// bounds (`clip_children_rect`) is listed by the box around its visible rect
/// and the part of that children's clip over the target, so what its children
/// paint past its bounds is covered too.
///
/// The walk ends at the widget carrying `stop_before_id`: it and everything
/// painted after it is left out (a host's own card layer, painted last).
/// Nothing is listed when no painted widget carries `target_id`, when nothing
/// of it shows, or when `stop_before_id` paints before it.
///
/// The list errs on the side of too much: a widget is listed by its whole
/// visible rect whether or not it paints all of it (a transparent panel lists
/// whole), so a spot clear of every rectangle listed is clear of every widget
/// painted over the target, as far as the host's `is_layer` is right. Not
/// covered: a widget's own `paint` runs under its parent's clip, not its
/// bounds, so a shadow or ring it draws past its bounds is outside the
/// rectangle listed; what a widget paints in `paint_overlay` after its
/// children (a scroll bar, a focus ring) and app-level overlays (tooltips,
/// popup menus) are not widgets in the tree and are not listed.
pub fn painted_over(
    root: &dyn Widget,
    target_id: &str,
    stop_before_id: Option<&str>,
    is_layer: impl Fn(&dyn Widget) -> bool,
) -> Vec<Rect> {
    painted_over_counted(root, target_id, stop_before_id, is_layer).0
}

/// [`painted_over`], with how many widgets the walk visited.
pub(crate) fn painted_over_counted(
    root: &dyn Widget,
    target_id: &str,
    stop_before_id: Option<&str>,
    is_layer: impl Fn(&dyn Widget) -> bool,
) -> (Vec<Rect>, usize) {
    let mut target: Option<Rect> = None;
    let mut over = Vec::new();
    let mut visited = 0usize;
    walk(root, &mut |widget, step| {
        visited += 1;
        if stop_before_id.is_some_and(|stop| widget.id() == Some(stop)) {
            return PaintWalk::Stop;
        }
        let own = widget.bounds();
        let shown = step
            .placement
            .visible_rect(Size::new(own.width, own.height));
        let Some(target) = target else {
            if widget.id() != Some(target_id) {
                return PaintWalk::Descend;
            }
            return match shown {
                Some(rect) => {
                    target = Some(rect);
                    PaintWalk::Descend
                }
                None => PaintWalk::Stop,
            };
        };
        // Where the children can paint over the target: their clip grown to
        // whole pixels, as each child's own clip is.
        let children_over = if widget.children().is_empty() {
            None
        } else {
            let clip = to_whole_pixels(step.children_clip, crate::device_scale());
            Some(intersect(clip, target)).filter(|r| has_area(*r))
        };
        match shown.filter(|rect| overlaps(*rect, target)) {
            Some(_) if is_layer(widget) => PaintWalk::Descend,
            Some(rect) => {
                over.push(match children_over {
                    Some(children) => bounding_box(rect, children),
                    None => rect,
                });
                PaintWalk::Skip
            }
            None if children_over.is_some() => PaintWalk::Descend,
            None => PaintWalk::Skip,
        }
    });
    (over, visited)
}

/// The window painting clips everything to, in the root's parent space (the
/// space of its `bounds()`): the root's bounds, cut to the part left on
/// screen while the on-screen keyboard lifts the tree.
pub(crate) fn painted_window(root: &dyn Widget) -> Rect {
    let b = root.bounds();
    let shown_root = super::visible_root_rect(Size::new(b.width, b.height));
    Rect::new(
        b.x + shown_root.x,
        b.y + shown_root.y,
        shown_root.width,
        shown_root.height,
    )
}

/// One visit: the placement the public walk hands out, and the exact
/// window-space clip the widget's children are under before any per-child
/// slot clip, the most of the window any of them can paint in place.
struct Step {
    placement: PaintPlacement,
    children_clip: Rect,
}

type Visit<'v> = dyn FnMut(&dyn Widget, &Step) -> PaintWalk + 'v;

/// The walk itself: the in-place pass, then the overlay pass.
fn walk(root: &dyn Widget, visit: &mut Visit<'_>) {
    let window = painted_window(root);
    let mut walker = Walker { window, visit };
    let on_entry = root_transform(root);
    if walker.in_place(root, on_entry, window, false).is_break() {
        return;
    }
    let _ = walker.overlay_pass(root, on_entry);
}

struct Walker<'a, 'v> {
    window: Rect,
    visit: &'a mut Visit<'v>,
}

impl Walker<'_, '_> {
    /// Visit `widget` and the subtree it paints in place, entered under
    /// `on_entry` (unsnapped) and the exact clip `clip`. `forced` marks the
    /// root of a subtree the overlay pass paints (its clip the window), which
    /// paints here although it defers.
    fn in_place(
        &mut self,
        widget: &dyn Widget,
        on_entry: TransAffine,
        clip: Rect,
        forced: bool,
    ) -> ControlFlow<()> {
        if !widget.is_visible() || widget.suppresses_paint() {
            return ControlFlow::Continue(());
        }
        // A deferring widget paints only as the root of its overlay-pass
        // subtree, entered at the window's clip.
        if widget.defer_paint_to_overlay() && !forced {
            return ControlFlow::Continue(());
        }
        let step = Step {
            placement: PaintPlacement {
                local_to_screen: entered(widget, &on_entry),
                clip: to_whole_pixels(clip, crate::device_scale()),
                visible_chain: true,
            },
            children_clip: intersect(clip, children_clip_rect(widget, &on_entry)),
        };
        match (self.visit)(widget, &step) {
            PaintWalk::Stop => return ControlFlow::Break(()),
            PaintWalk::Skip => return ControlFlow::Continue(()),
            PaintWalk::Descend => {}
        }
        let children_to_screen = children_transform(widget, &on_entry);
        let children = widget.children();
        // Children drawn on top of their siblings paint after the rest.
        for on_top in [false, true] {
            for (index, child) in children.iter().enumerate() {
                let child = child.as_ref();
                if child.draw_on_top_of_siblings() != on_top {
                    continue;
                }
                let child_clip = match widget.child_paint_clip(index) {
                    Some(slot) => intersect(
                        step.children_clip,
                        super::paint_geometry::screen_aabb(&children_to_screen, slot),
                    ),
                    None => step.children_clip,
                };
                let child_entry = offset_child(child, &children_to_screen);
                self.in_place(child, child_entry, child_clip, false)?;
            }
        }
        ControlFlow::Continue(())
    }

    /// The overlay pass: every painted widget in post-order, list order,
    /// each one that defers painting its subtree at the window's clip.
    fn overlay_pass(&mut self, widget: &dyn Widget, on_entry: TransAffine) -> ControlFlow<()> {
        if !widget.is_visible() || widget.suppresses_paint() {
            return ControlFlow::Continue(());
        }
        let children_to_screen = children_transform(widget, &on_entry);
        for child in widget.children() {
            let child = child.as_ref();
            self.overlay_pass(child, offset_child(child, &children_to_screen))?;
        }
        if widget.defer_paint_to_overlay() {
            self.in_place(widget, on_entry, self.window, true)?;
        }
        ControlFlow::Continue(())
    }
}

/// The two rectangles share an area, not only an edge.
fn overlaps(a: Rect, b: Rect) -> bool {
    has_area(intersect(a, b))
}

fn has_area(r: Rect) -> bool {
    r.width > 0.0 && r.height > 0.0
}

/// The smallest rectangle holding both.
fn bounding_box(a: Rect, b: Rect) -> Rect {
    let left = a.left().min(b.left());
    let bottom = a.bottom().min(b.bottom());
    let right = a.right().max(b.right());
    let top = a.top().max(b.top());
    Rect::new(left, bottom, right - left, top - bottom)
}
