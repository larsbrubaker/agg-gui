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
//! uses the origin.  Margins are kept on anchored edges, as in C#.  A
//! fit-or-stretch child (`FIT | STRETCH`) stretches but never below the
//! size it reports.  The engine itself is [`simple_align`](super::simple_align),
//! shared with [`GroupBox`](super::GroupBox).
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
/// that axis, their margins and its padding on both sides included (C#
/// `DoFitToChildrenHorizontal`/`Vertical`), and the children are then placed
/// again against the fitted size: an un-anchored child at origin 0 sits its
/// margin and the padding above the fitted bottom edge, as in C#.
pub struct AbsoluteLayout {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    padding: Insets,
    /// C#'s `LocalBounds` low corner (see `simple_align`).
    local_low: Point,
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
            local_low: Point::ORIGIN,
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

    /// C#'s `LocalBounds`: the container's size, starting at the low
    /// corner fitting moved it to (see [`simple_align`](super::simple_align)).
    /// The corner is `(0, 0)` unless a FIT axis has children (or padding)
    /// below or left of the origin.
    pub fn local_bounds(&self) -> Rect {
        Rect::new(
            self.local_low.x,
            self.local_low.y,
            self.bounds.width,
            self.bounds.height,
        )
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
    /// The inspector's padding edits (C# `Padding = …`).
    fn try_set_padding(&mut self, padding: Insets) -> bool {
        self.padding = padding;
        true
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
        if let Some(size) = crate::shared_size_group::layout_as_scope(self, available) {
            return size;
        }
        super::simple_align::layout_simple_align(
            &mut self.children,
            &self.base,
            self.padding,
            &mut self.local_low,
            available,
        )
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
