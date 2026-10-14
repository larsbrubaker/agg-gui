//! Where painting puts a widget, and what of it painting lets show: the
//! transform and clip chain of `agg_gui::widget::paint`
//! (`paint_subtree_direct_inner` and `paint_child`), replayed without a draw
//! context so the name lookups ([`crate::tree_query`], [`crate::name_pick`])
//! and the painted frame agree at every clip edge.
//!
//! The rules followed, each as painting applies it:
//! - a child is clipped and offset by its exact `bounds()` origin in its
//!   parent's exact (unsnapped) transform, and a widget that enforces integer bounds
//!   paints itself with the translation moved to the nearest whole device
//!   pixel (`DrawCtx::snap_to_pixel`), so snaps never compound;
//! - a widget's children are clipped to its `clip_children_rect` (its bounds
//!   by default), then offset by its child transform (pan/zoom; the
//!   inspector's [`Widget::inspector_child_transform`], which defaults to
//!   `child_transform`), and each child is clipped by the parent's
//!   `child_paint_clip` for its slot (a `FlexRow` column);
//! - a subtree that paints in the global overlay pass
//!   (`defer_paint_to_overlay`, a modal hosted inside a clipped container)
//!   escapes every ancestor's clip and is clipped by the window alone.
//!
//! Rectangles are agg-gui's native space: logical units, Y-up, the root's
//! lower-left corner at the window's.

use agg_gui::{Rect, Size, TransAffine, Widget};

/// The walk's result for one widget: how its own local space maps to the
/// window, the clip its own paint is under, and whether it and every
/// ancestor are visible.
#[derive(Clone, Copy, Debug)]
pub struct PaintPlacement {
    /// The widget's local space (origin at its lower-left corner) to window
    /// space, as painting's transform stands when the widget paints itself.
    pub local_to_screen: TransAffine,
    /// The window-space clip the widget paints under: every enclosing clip
    /// and the window.
    pub clip: Rect,
    /// Every widget from the root down to this one reports `is_visible()`.
    pub visible_chain: bool,
}

impl PaintPlacement {
    /// The widget's own rectangle (`width` × `height` from its origin) in
    /// window space.
    pub fn screen_rect(&self, size: Size) -> Rect {
        screen_aabb(
            &self.local_to_screen,
            Rect::new(0.0, 0.0, size.width, size.height),
        )
    }
}

/// Follow `path` from `root`, replaying painting's transforms and clips (see
/// the module docs). `viewport` is the window's logical size. `None` when
/// the path leaves the tree.
pub fn paint_placement(
    root: &dyn Widget,
    path: &[usize],
    viewport: Size,
) -> Option<PaintPlacement> {
    let window = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    let mut node = root;
    let mut on_entry = root_transform(root);
    let mut clip = window;
    let mut visible_chain = node.is_visible();
    for &index in path {
        if node.defer_paint_to_overlay() {
            clip = window;
        }
        // Children are clipped and offset in the exact transform, not the
        // snapped one (each snaps itself on entering).
        let children_clip = intersect(clip, children_clip_rect(node, &on_entry));
        let children_to_screen = children_transform(node, &on_entry);
        let child = node.children().get(index)?.as_ref();
        clip = match node.child_paint_clip(index) {
            Some(slot) => intersect(children_clip, screen_aabb(&children_to_screen, slot)),
            None => children_clip,
        };
        on_entry = offset_child(child, &children_to_screen);
        node = child;
        visible_chain &= node.is_visible();
    }
    if node.defer_paint_to_overlay() {
        clip = window;
    }
    Some(PaintPlacement {
        local_to_screen: entered(node, &on_entry),
        clip,
        visible_chain,
    })
}

/// The window-space rectangle `widget` clips its children to
/// (`clip_children_rect`, its bounds when it returns `None`).
fn children_clip_rect(widget: &dyn Widget, local_to_screen: &TransAffine) -> Rect {
    let b = widget.bounds();
    let (x, y, w, h) = widget
        .clip_children_rect()
        .unwrap_or((0.0, 0.0, b.width, b.height));
    screen_aabb(local_to_screen, Rect::new(x, y, w, h))
}

/// The paint transform on entering the root: the root paints at its own
/// offset, as every child does.
pub fn root_transform(root: &dyn Widget) -> TransAffine {
    let b = root.bounds();
    TransAffine::new_translation(b.x, b.y)
}

/// The transform `widget` itself paints under, given the one on entering it:
/// snapped onto the pixel grid when it enforces integer bounds
/// (`paint_subtree_direct_inner`'s `snap_this`).
pub fn entered(widget: &dyn Widget, on_entry: &TransAffine) -> TransAffine {
    let mut t = *on_entry;
    if widget.enforce_integer_bounds() {
        snap_to_pixel(&mut t);
    }
    t
}

/// The transform `widget`'s children are offset within: its own, then its
/// child transform (pan/zoom), which painting applies after the children
/// clip.
pub fn children_transform(widget: &dyn Widget, local_to_screen: &TransAffine) -> TransAffine {
    let mut t = *local_to_screen;
    t.premultiply(&widget.inspector_child_transform());
    t
}

/// The transform on entering `child`: its exact offset within its parent's
/// children space (painting snaps it on entering, see [`entered`]), applied
/// as `DrawCtx::translate` does (pre-multiplied, so in the children's space).
pub fn offset_child(child: &dyn Widget, children_to_screen: &TransAffine) -> TransAffine {
    let b = child.bounds();
    let mut t = *children_to_screen;
    t.premultiply(&TransAffine::new_translation(b.x, b.y));
    t
}

/// `DrawCtx::snap_to_pixel` on a transform: move its translation to the
/// nearest whole pixel through the same pre-multiplied `translate` step the
/// paint context takes (agg-gui's `draw_ctx::defaults::pixel_snap_step`,
/// which divides by the scale, so under a zoomed parent the origin lands on
/// the pixel rather than a fraction of the zoom past it). The walk is in
/// logical units, so this is the device grid at a device scale of 1
/// (headless test windows); at other scales a snapped widget can sit up to
/// a pixel away from where it paints.
fn snap_to_pixel(t: &mut TransAffine) {
    if let Some((dx, dy)) = agg_gui::draw_ctx::defaults::pixel_snap_step(t) {
        t.premultiply(&TransAffine::new_translation(dx, dy));
    }
}

/// The overlap of two rectangles; zero-sized at the nearer edge when they do
/// not overlap.
pub fn intersect(a: Rect, b: Rect) -> Rect {
    let left = a.left().max(b.left());
    let bottom = a.bottom().max(b.bottom());
    let right = a.right().min(b.right());
    let top = a.top().min(b.top());
    Rect::new(
        left,
        bottom,
        (right - left).max(0.0),
        (top - bottom).max(0.0),
    )
}

/// `rect` through `to_screen`, as the axis-aligned box around its corners
/// (exact for translation plus scale).
pub fn screen_aabb(to_screen: &TransAffine, rect: Rect) -> Rect {
    let corners = [
        (rect.left(), rect.bottom()),
        (rect.right(), rect.bottom()),
        (rect.left(), rect.top()),
        (rect.right(), rect.top()),
    ];
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (mut x, mut y) in corners {
        to_screen.transform(&mut x, &mut y);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    Rect::new(
        min_x,
        min_y,
        (max_x - min_x).max(0.0),
        (max_y - min_y).max(0.0),
    )
}
