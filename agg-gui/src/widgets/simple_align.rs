//! The layout engine shared by [`AbsoluteLayout`](super::AbsoluteLayout) and
//! [`GroupBox`](super::GroupBox): C#'s `LayoutEngineSimpleAlign` (anchor each
//! child, fit the parent to its children, anchor again) with `GuiWidget`'s
//! `GetMinimumBoundsToEncloseChildren(true)`.
//!
//! C#'s `LocalBounds` can start below or left of the widget's origin: fitting
//! sets `LocalBounds.Bottom`/`Left` to the low edge of the children (margins
//! and padding included), so a child at origin 0 with a margin sits a margin
//! and the padding above the fitted parent's bottom edge. agg-gui bounds
//! start at the widget's bottom-left, so the engine keeps that low edge as a
//! `local_low` offset: an un-anchored child at C# origin `o` is placed at
//! `o - local_low`, anchored children are placed against the edges as
//! before, and the container reports C#'s `LocalBounds` through it.

use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

/// One axis of a child's anchor, reduced to the three C# position bits.
#[derive(Clone, Copy)]
struct AxisAnchor {
    low: bool,
    center: bool,
    high: bool,
}

impl AxisAnchor {
    fn h(a: HAnchor) -> Self {
        Self {
            low: a.contains(HAnchor::LEFT),
            center: a.contains(HAnchor::CENTER),
            high: a.contains(HAnchor::RIGHT),
        }
    }
    fn v(a: VAnchor) -> Self {
        Self {
            low: a.contains(VAnchor::BOTTOM),
            center: a.contains(VAnchor::CENTER),
            high: a.contains(VAnchor::TOP),
        }
    }
    /// C# `HAnchorIsFloating` / `VAnchorIsFloating`: exactly one of the
    /// three position bits, so the child's position follows the parent's
    /// size and only its size can hold the parent open.
    fn floating(self) -> bool {
        u8::from(self.low) + u8::from(self.center) + u8::from(self.high) == 1
    }
}

/// One axis of the parent's space and the child's request on it.
struct AxisSlot {
    parent: f64,
    pad_low: f64,
    pad_high: f64,
    margin_low: f64,
    margin_high: f64,
    origin: f64,
    natural: f64,
}

/// Lay `children` out against `available` the way a C# `GuiWidget` with
/// `LayoutEngineSimpleAlign` does, fitting on the axes `base` asks to fit.
/// Returns the container's size and leaves C#'s `LocalBounds` low corner in
/// `local_low`.
pub(crate) fn layout_simple_align(
    children: &mut [Box<dyn Widget>],
    base: &WidgetBase,
    padding: Insets,
    local_low: &mut Point,
    available: Size,
) -> Size {
    // A child without a `WidgetBase` keeps its position: read it in C#
    // local coordinates through the low corner it was placed with.
    let placed_with = std::mem::replace(local_low, Point::ORIGIN);
    place_children(children, padding, placed_with, Point::ORIGIN, available);
    let fit_w = base.h_anchor.contains(HAnchor::FIT) && !base.h_anchor.is_stretch();
    let fit_h = base.v_anchor.contains(VAnchor::FIT) && !base.v_anchor.is_stretch();
    // FIT | STRETCH (`MAX_FIT_OR_STRETCH`): the larger of the two, so the
    // parent's size when it has room and the children's when it does not
    // (C# `ParentStretchAndFitToChildren`).
    let max_w = base.h_anchor == HAnchor::MAX_FIT_OR_STRETCH;
    let max_h = base.v_anchor == VAnchor::MAX_FIT_OR_STRETCH;
    if !fit_w && !fit_h && !max_w && !max_h {
        return available;
    }
    let enclosing = enclosing_bounds(children, padding);
    // (low edge, size) on one axis: C#'s `DoFitToChildrenHorizontal` /
    // `Vertical` move `LocalBounds`' low edge to the children's only when
    // they set the size from the children.
    let axis = |fit: bool, max: bool, low: f64, size: f64, available: f64| {
        if fit || (max && size > available) {
            (low, size)
        } else {
            (0.0, available)
        }
    };
    let (low_x, w) = axis(fit_w, max_w, enclosing.x, enclosing.width, available.width);
    let (low_y, h) = axis(
        fit_h,
        max_h,
        enclosing.y,
        enclosing.height,
        available.height,
    );
    let size = base.clamp_size(Size::new(w, h));
    let low = Point::new(low_x, low_y);
    if size != available || low != Point::ORIGIN {
        // C# re-anchors the children when fitting changed the parent's
        // size (LayoutEngineSimpleAlign.Layout's second pass).
        *local_low = low;
        place_children(children, padding, Point::ORIGIN, low, size);
    }
    size
}

/// Lay out and place every visible child against a `size` parent whose C#
/// `LocalBounds` start at `low`; `placed_with` is the low corner the
/// children's current bounds were placed with.
fn place_children(
    children: &mut [Box<dyn Widget>],
    pad: Insets,
    placed_with: Point,
    low: Point,
    size: Size,
) {
    for child in children.iter_mut().filter(|c| c.is_visible()) {
        let m = child.margin();
        let origin = child.widget_base().map(|b| b.origin).unwrap_or_else(|| {
            Point::new(
                child.bounds().x + placed_with.x,
                child.bounds().y + placed_with.y,
            )
        });
        let room = Size::new(
            (size.width - pad.horizontal() - m.horizontal()).max(0.0),
            (size.height - pad.vertical() - m.vertical()).max(0.0),
        );
        let natural = child.layout(room);
        let (min, max) = (child.min_size(), child.max_size());
        let ha = child.h_anchor();
        let va = child.v_anchor();
        let (x, w) = place_axis(
            AxisAnchor::h(ha),
            AxisSlot {
                parent: size.width,
                pad_low: pad.left,
                pad_high: pad.right,
                margin_low: m.left,
                margin_high: m.right,
                origin: origin.x - low.x,
                natural: natural.width.clamp(min.width, max.width),
            },
        );
        let (y, h) = place_axis(
            AxisAnchor::v(va),
            AxisSlot {
                parent: size.height,
                pad_low: pad.bottom,
                pad_high: pad.top,
                margin_low: m.bottom,
                margin_high: m.top,
                origin: origin.y - low.y,
                natural: natural.height.clamp(min.height, max.height),
            },
        );
        // A fit-or-stretch child (C# `Fit | Stretch`) never shrinks below
        // its content: C#'s `DoFitToChildrenHorizontal`/`Vertical` take the
        // larger of the parent's size and the children's.
        let w = if ha.contains(HAnchor::FIT) && ha.is_stretch() {
            w.max(natural.width)
        } else {
            w
        };
        let h = if va.contains(VAnchor::FIT) && va.is_stretch() {
            h.max(natural.height)
        } else {
            h
        };
        let w = w.clamp(min.width, max.width).max(0.0);
        let h = h.clamp(min.height, max.height).max(0.0);
        if w != room.width || h != room.height {
            // The child arranged its own children against `room`; re-lay
            // it at the box it actually occupies.  Comparing with the
            // measured `room` (not the reported `natural`) matters for a
            // fitted child: a content-fitted `FlexColumn` reports less
            // than `room` but placed its rows from the top (and centred
            // them) within `room`, so without this pass they would sit
            // above/right of its box and be clipped away.
            child.layout(Size::new(w, h));
        }
        // C# `OriginRelativeParent`'s setter rounds with Math.Round
        // (banker's) when EnforceIntegerBounds is on.
        let integer = child
            .widget_base()
            .map(|b| b.enforce_integer_bounds)
            .unwrap_or_else(|| child.enforce_integer_bounds());
        let (x, y) = if integer {
            (round_half_even(x), round_half_even(y))
        } else {
            (x, y)
        };
        child.set_bounds(Rect::new(x, y, w, h));
    }
}

/// C# `GetMinimumBoundsToEncloseChildren(true)` in the container's C#
/// local coordinates (children are placed with `local_low` at the origin
/// when this runs): the box of the children that can't move, margins
/// included, at least as large as the largest child plus its margins, then
/// grown by the padding. A child that stretches on an axis (anchor exactly
/// `STRETCH`) does not count on it: it takes the parent's size rather than
/// giving one. A floating child (one position bit) counts only by its size.
fn enclosing_bounds(children: &[Box<dyn Widget>], pad: Insets) -> Rect {
    let mut visible = children.iter().filter(|c| c.is_visible()).peekable();
    if visible.peek().is_none() {
        return Rect::new(-pad.left, -pad.bottom, pad.horizontal(), pad.vertical());
    }
    let (mut left, mut right) = (f64::MAX, f64::MIN);
    let (mut bottom, mut top) = (f64::MAX, f64::MIN);
    let (mut min_w, mut min_h): (f64, f64) = (0.0, 0.0);
    let (mut found_h, mut found_v) = (false, false);
    for child in visible {
        let b = child.bounds();
        let m = child.margin();
        let (ha, va) = (child.h_anchor(), child.v_anchor());
        if ha != HAnchor::STRETCH {
            min_w = min_w.max(b.width + m.horizontal());
            if !AxisAnchor::h(ha).floating() {
                found_h = true;
                left = left.min(b.x - m.left);
                right = right.max(b.x + b.width + m.right);
            }
        }
        if va != VAnchor::STRETCH {
            min_h = min_h.max(b.height + m.vertical());
            if !AxisAnchor::v(va).floating() {
                found_v = true;
                bottom = bottom.min(b.y - m.bottom);
                top = top.max(b.y + b.height + m.top);
            }
        }
    }
    let (left, width) = if found_h {
        (left, (right - left).max(min_w))
    } else {
        (0.0, min_w)
    };
    let (bottom, height) = if found_v {
        (bottom, (top - bottom).max(min_h))
    } else {
        (0.0, min_h)
    };
    Rect::new(
        left - pad.left,
        bottom - pad.bottom,
        width + pad.horizontal(),
        height + pad.vertical(),
    )
}

/// C#'s `GetOriginAndWidthForChild` / `GetOriginAndHeightForChild` for one
/// axis: returns the child's position and size on it.
fn place_axis(anchor: AxisAnchor, slot: AxisSlot) -> (f64, f64) {
    let usable = slot.parent - (slot.pad_low + slot.pad_high);
    let margins = slot.margin_low + slot.margin_high;
    if anchor.low {
        let pos = slot.pad_low + slot.margin_low;
        if anchor.center {
            (pos, usable / 2.0 - margins)
        } else if anchor.high {
            (pos, usable - margins)
        } else {
            (pos, slot.natural)
        }
    } else if anchor.center {
        if anchor.high {
            (
                slot.pad_low + slot.margin_low + usable / 2.0,
                usable / 2.0 - margins,
            )
        } else {
            let center = slot.pad_low + usable / 2.0;
            (
                center - (slot.natural + margins) / 2.0 + slot.margin_low,
                slot.natural,
            )
        }
    } else if anchor.high {
        (
            slot.parent - slot.margin_high - slot.pad_high - slot.natural,
            slot.natural,
        )
    } else {
        (slot.origin, slot.natural)
    }
}

/// C# `Math.Round`: round half to even (`f64::round_ties_even` needs Rust
/// 1.77; agg-gui's MSRV is 1.70).
fn round_half_even(v: f64) -> f64 {
    let r = v.round();
    if (v - v.trunc()).abs() == 0.5 {
        2.0 * (v / 2.0).round()
    } else {
        r
    }
}
