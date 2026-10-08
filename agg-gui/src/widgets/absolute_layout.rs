//! `AbsoluteLayout` — a container that places each child where the child
//! asks to be: the counterpart of a plain C# `GuiWidget` parent, whose
//! children keep their `OriginRelativeParent` (`new Button("left", 10, 40)`)
//! unless an anchor moves them.
//!
//! Each child's position comes from its [`WidgetBase::origin`] (set with
//! [`Widget::with_origin`]); a child without a `WidgetBase` keeps the origin
//! of its current bounds, so a widget that positions itself stays put.
//! Anchors follow C#'s `LayoutEngineSimpleAlign` per axis
//! (`GetOriginAndWidthForChild` / `GetOriginAndHeightForChild`): LEFT/BOTTOM
//! hold the child to the padded edge, RIGHT/TOP to the far edge, CENTER
//! centres it, LEFT|RIGHT stretches it, and LEFT|CENTER or CENTER|RIGHT give
//! it the matching half.  An axis with none of those bits (FIT, ABSOLUTE)
//! uses the origin.  Margins are kept on anchored edges, as in C#.
//!
//! Siblings: [`Stack`](super::Stack) overlays children at the same place,
//! [`FlexColumn`](super::FlexColumn)/[`FlexRow`](super::FlexRow) flow them.

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

/// Children placed by origin and anchors (C# `GuiWidget` with
/// `LayoutEngineSimpleAlign`).
///
/// The container itself stretches to the slot it is given by default; with
/// a FIT anchor on an axis it shrinks to enclose its visible children on
/// that axis (C# `DoFitToChildrenHorizontal`/`Vertical`), and anchored
/// children are then placed again against the fitted size.
pub struct AbsoluteLayout {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    padding: Insets,
}

/// One axis of a child's anchor, reduced to the three C# position bits.
#[derive(Clone, Copy)]
struct AxisAnchor {
    low: bool,
    center: bool,
    high: bool,
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

impl AbsoluteLayout {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new()
                .with_h_anchor(HAnchor::STRETCH)
                .with_v_anchor(VAnchor::STRETCH),
            padding: Insets::ZERO,
        }
    }

    /// Add a child; it is placed at its own origin (see the module docs).
    // Named like `Stack::add` / `FlexColumn::add`, the crate's builder idiom.
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self
    }

    /// Space kept clear inside the edges for anchored children (C#
    /// `Padding`).
    pub fn with_padding(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }
    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }

    /// Lay out and place every visible child against a `size` parent.
    fn place_children(&mut self, size: Size) {
        let pad = self.padding;
        for child in self.children.iter_mut().filter(|c| c.is_visible()) {
            let m = child.margin();
            let origin = child
                .widget_base()
                .map(|b| b.origin)
                .unwrap_or_else(|| Point::new(child.bounds().x, child.bounds().y));
            let room = Size::new(
                (size.width - pad.horizontal() - m.horizontal()).max(0.0),
                (size.height - pad.vertical() - m.vertical()).max(0.0),
            );
            let natural = child.layout(room);
            let (min, max) = (child.min_size(), child.max_size());
            let ha = child.h_anchor();
            let va = child.v_anchor();
            let (x, w) = place_axis(
                AxisAnchor {
                    low: ha.contains(HAnchor::LEFT),
                    center: ha.contains(HAnchor::CENTER),
                    high: ha.contains(HAnchor::RIGHT),
                },
                AxisSlot {
                    parent: size.width,
                    pad_low: pad.left,
                    pad_high: pad.right,
                    margin_low: m.left,
                    margin_high: m.right,
                    origin: origin.x,
                    natural: natural.width.clamp(min.width, max.width),
                },
            );
            let (y, h) = place_axis(
                AxisAnchor {
                    low: va.contains(VAnchor::BOTTOM),
                    center: va.contains(VAnchor::CENTER),
                    high: va.contains(VAnchor::TOP),
                },
                AxisSlot {
                    parent: size.height,
                    pad_low: pad.bottom,
                    pad_high: pad.top,
                    margin_low: m.bottom,
                    margin_high: m.top,
                    origin: origin.y,
                    natural: natural.height.clamp(min.height, max.height),
                },
            );
            let w = w.clamp(min.width, max.width).max(0.0);
            let h = h.clamp(min.height, max.height).max(0.0);
            if w != natural.width || h != natural.height {
                // Re-lay the child at the size it was given so its own
                // content fills the box it actually occupies.
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

    /// The far edges of the visible children, margins and padding included
    /// (C# `GetMinimumBoundsToEncloseChildren(true)` measured from 0).
    fn enclosing_size(&self) -> Size {
        let mut right: f64 = 0.0;
        let mut top: f64 = 0.0;
        for child in self.children.iter().filter(|c| c.is_visible()) {
            let b = child.bounds();
            let m = child.margin();
            right = right.max(b.x + b.width + m.right);
            top = top.max(b.y + b.height + m.top);
        }
        Size::new(right + self.padding.right, top + self.padding.top)
    }
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

impl Default for AbsoluteLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for AbsoluteLayout {
    fn type_name(&self) -> &'static str {
        "AbsoluteLayout"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn padding(&self) -> Insets {
        self.padding
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn min_size(&self) -> Size {
        self.base.min_size
    }
    fn max_size(&self) -> Size {
        self.base.max_size
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn enforce_integer_bounds(&self) -> bool {
        self.base.enforce_integer_bounds
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn layout(&mut self, available: Size) -> Size {
        self.place_children(available);
        let fit_w = self.base.h_anchor.contains(HAnchor::FIT) && !self.base.h_anchor.is_stretch();
        let fit_h = self.base.v_anchor.contains(VAnchor::FIT) && !self.base.v_anchor.is_stretch();
        if !fit_w && !fit_h {
            return available;
        }
        let enclosing = self.enclosing_size();
        let size = self.base.clamp_size(Size::new(
            if fit_w {
                enclosing.width
            } else {
                available.width
            },
            if fit_h {
                enclosing.height
            } else {
                available.height
            },
        ));
        if size != available {
            // C# re-anchors the children when fitting changed the parent's
            // size (LayoutEngineSimpleAlign.Layout's second pass).
            self.place_children(size);
        }
        size
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
