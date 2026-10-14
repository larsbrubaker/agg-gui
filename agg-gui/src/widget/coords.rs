//! Coordinate conversion between a widget's own space and an ancestor's (or
//! the screen's), through every offset and every zoom between them.
//!
//! Port of agg-sharp's `GuiWidget.TransformToParentSpace` /
//! `TransformFromParentSpace` / `TransformToScreenSpace` /
//! `TransformFromScreenSpace` (`Gui/GUIWidget.cs`) and the safe affine
//! helpers they use (`TransformedPoint`, `InverseTransformedPoint`,
//! `TransformedBounds`). agg-gui widgets hold no parent pointer, so a widget
//! is named by its child-index `path` from a `root` (as [`walk_path`] and
//! hit-testing name it), and the space to convert to or from by a
//! [`CoordSpace`].
//!
//! The chain is the one painting, hit-testing ([`child_local_pos`]) and
//! [`event_root`](super::event_root) follow: a point in a widget's space is
//! offset by the widget's `bounds()` origin, then mapped through its
//! parent's [`Widget::child_transform`] (a pan/zoom canvas such as the node
//! editor's card layer), and so on up. Screen space is the root's parent's:
//! the root's own `bounds()` origin is applied too, as
//! [`find_widget_screen_rect`](super::find_widget_screen_rect) treats it.
//!
//! [`walk_path`]: super::walk_path
//! [`child_local_pos`]: super::tree

use super::Widget;
use crate::geometry::{Point, Rect};
use crate::TransAffine;

/// The space a conversion goes to or comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordSpace {
    /// The root's parent space: the root's `bounds()` origin applied
    /// (C#'s screen space, `TransformToScreenSpace`).
    Screen,
    /// The local space of the ancestor at `path[..depth]` (`0` is the root
    /// itself), before that ancestor's own offset. C#'s
    /// `parentToGetRelativeTo`. A depth of `path.len()` is the widget's own
    /// space (no conversion).
    Ancestor(usize),
}

/// The transforms between the widget at `path` and `space`, innermost first:
/// each widget's bounds offset (a pure translation) and its parent's child
/// transform, alternating up the tree.
fn chain(root: &dyn Widget, path: &[usize], space: CoordSpace) -> Option<Vec<TransAffine>> {
    let (stop, include_root) = match space {
        CoordSpace::Screen => (0, true),
        CoordSpace::Ancestor(depth) if depth <= path.len() => (depth, false),
        CoordSpace::Ancestor(_) => return None,
    };
    // Every widget from the root down to the target.
    let mut nodes: Vec<&dyn Widget> = Vec::with_capacity(path.len() + 1);
    let mut node = root;
    nodes.push(node);
    for &index in path {
        node = node.children().get(index)?.as_ref();
        nodes.push(node);
    }
    // Innermost first: the widget's own offset, then its parent's child
    // transform, then the parent's offset, ... up to the ancestor.
    let mut steps = Vec::new();
    for level in (stop + 1..nodes.len()).rev() {
        let b = nodes[level].bounds();
        steps.push(TransAffine::new_translation(b.x, b.y));
        if let Some(t) = nodes[level - 1].child_transform() {
            steps.push(t);
        }
    }
    if include_root {
        let b = root.bounds();
        steps.push(TransAffine::new_translation(b.x, b.y));
    }
    Some(steps)
}

/// `p` in the widget at `path`'s own space, in `space` — where it is drawn
/// under every zoomed ancestor. `None` when `path` leaves the tree or
/// `space` is not an ancestor of it.
pub fn point_to(root: &dyn Widget, path: &[usize], space: CoordSpace, p: Point) -> Option<Point> {
    let steps = chain(root, path, space)?;
    Some(steps.iter().fold(p, |p, t| transformed_point(t, p)))
}

/// `p` in `space`, in the widget at `path`'s own space: the exact inverse of
/// [`point_to`], undoing the outermost step first.
pub fn point_from(root: &dyn Widget, path: &[usize], space: CoordSpace, p: Point) -> Option<Point> {
    let steps = chain(root, path, space)?;
    Some(
        steps
            .iter()
            .rev()
            .fold(p, |p, t| inverse_transformed_point(t, p)),
    )
}

/// `rect` in the widget at `path`'s own space, as the axis-aligned bounds it
/// covers in `space`.
pub fn rect_to(root: &dyn Widget, path: &[usize], space: CoordSpace, rect: Rect) -> Option<Rect> {
    let steps = chain(root, path, space)?;
    Some(steps.iter().fold(rect, |r, t| transformed_bounds(t, r)))
}

/// `rect` in `space`, as the axis-aligned bounds it covers in the widget at
/// `path`'s own space: the inverse of [`rect_to`].
pub fn rect_from(root: &dyn Widget, path: &[usize], space: CoordSpace, rect: Rect) -> Option<Rect> {
    let steps = chain(root, path, space)?;
    Some(steps.iter().rev().fold(rect, |r, t| {
        let mut inverse = *t;
        inverse.invert();
        transformed_bounds(&inverse, r)
    }))
}

/// `term * value`, or nothing for a zero term: in IEEE infinity times zero is
/// NaN, and an unbounded coordinate under a shrinking ancestor is infinite.
/// A zero term contributes nothing, so it is skipped (C#
/// `TransformedPoint`).
fn product(value: f64, term: f64) -> f64 {
    if term == 0.0 {
        0.0
    } else {
        value * term
    }
}

/// `p` mapped through `t`, staying finite where a full matrix multiply would
/// not. Without shear the axes are mapped separately, which keeps a pure
/// translation exactly `x + tx`.
fn transformed_point(t: &TransAffine, p: Point) -> Point {
    if t.shx == 0.0 && t.shy == 0.0 {
        return Point::new(product(p.x, t.sx) + t.tx, product(p.y, t.sy) + t.ty);
    }
    Point::new(
        product(p.x, t.sx) + product(p.y, t.shx) + t.tx,
        product(p.x, t.shy) + product(p.y, t.sy) + t.ty,
    )
}

/// `p` mapped back through `t`, the inverse of [`transformed_point`].
///
/// A transform with a zero scale (a collapsed widget) has no inverse. Every
/// point on a collapsed axis is drawn at the same place, so any answer is as
/// good as another; that axis falls back to removing the translation only,
/// which keeps a caller's arithmetic finite. Without shear the axes are undone
/// separately (a pure translation is exactly `x - tx`); a singular sheared
/// transform falls back the same way on both axes.
fn inverse_transformed_point(t: &TransAffine, p: Point) -> Point {
    if t.shx == 0.0 && t.shy == 0.0 {
        return Point::new(
            if t.sx == 0.0 {
                p.x - t.tx
            } else {
                (p.x - t.tx) / t.sx
            },
            if t.sy == 0.0 {
                p.y - t.ty
            } else {
                (p.y - t.ty) / t.sy
            },
        );
    }
    let determinant = t.sx * t.sy - t.shy * t.shx;
    if determinant == 0.0 || !determinant.is_finite() {
        return Point::new(p.x - t.tx, p.y - t.ty);
    }
    let mut inverse = *t;
    inverse.invert();
    transformed_point(&inverse, p)
}

/// The axis-aligned bounds `rect` covers under `t`.
///
/// An inverted rectangle (negative width or height) means "nothing", so it
/// comes out inverted too: without rotation or shear an axis's edges swap
/// only when that axis's scale flips it (which also keeps a pure translation
/// exactly the `x + tx` offset); a rotated or sheared transform returns an
/// empty input as an inverted rectangle, since a corner walk would rebuild it
/// as a valid one.
fn transformed_bounds(t: &TransAffine, rect: Rect) -> Rect {
    let (l, b, r, top) = (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
    if t.shx == 0.0 && t.shy == 0.0 {
        let (mut left, mut right) = (l * t.sx + t.tx, r * t.sx + t.tx);
        if t.sx < 0.0 {
            std::mem::swap(&mut left, &mut right);
        }
        let (mut bottom, mut top) = (b * t.sy + t.ty, top * t.sy + t.ty);
        if t.sy < 0.0 {
            std::mem::swap(&mut bottom, &mut top);
        }
        return Rect::new(left, bottom, right - left, top - bottom);
    }
    if rect.width < 0.0 || rect.height < 0.0 {
        return Rect::new(0.0, 0.0, -1.0, -1.0);
    }
    let (mut x0, mut y0) = (f64::INFINITY, f64::INFINITY);
    let (mut x1, mut y1) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (x, y) in [(l, b), (r, b), (r, top), (l, top)] {
        let p = transformed_point(t, Point::new(x, y));
        // Infinity minus infinity at a corner with both coordinates infinite
        // is NaN; the opposite corner already carries that axis out, so a
        // NaN coordinate is ignored (C# `ExpandToInclude`).
        if !p.x.is_nan() {
            x0 = x0.min(p.x);
            x1 = x1.max(p.x);
        }
        if !p.y.is_nan() {
            y0 = y0.min(p.y);
            y1 = y1.max(p.y);
        }
    }
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
