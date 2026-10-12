//! `GroupBox` — a titled frame around a client area (agg-sharp
//! `Gui/GroupBox.cs`).
//!
//! The box fits its children on both axes (C# `HAnchor.Fit` /
//! `VAnchor.Fit`) with the same engine as
//! [`AbsoluteLayout`](super::AbsoluteLayout)
//! ([`simple_align`](super::simple_align)). Its children are the title
//! label, anchored top-left and lifted into the top padding by a negative
//! top margin, and the client area, a `MAX_FIT_OR_STRETCH` `AbsoluteLayout`
//! that [`GroupBox::add`] puts content into (C#'s `AddChild` override). The
//! frame is a rectangle inset 8.5 device pixels, broken around the label.

use std::sync::Arc;

use super::{AbsoluteLayout, Label};
use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

/// A titled frame (C# `GroupBox`): children are `[label, client area]`.
pub struct GroupBox {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    padding: Insets,
    /// C#'s `LocalBounds` low corner (see `simple_align`).
    local_low: Point,
    border_color: Color,
}

/// Index of the client area in `children`.
const CLIENT_AREA: usize = 1;

impl GroupBox {
    /// A group box titled with a [`Label`] (C# `new GroupBox(title)`).
    pub fn new(title: impl Into<String>, font: Arc<Font>) -> Self {
        Self::with_label(Box::new(Label::new(title, font)))
    }

    /// A group box titled by any widget (C# `new GroupBox(GuiWidget)`):
    /// the label takes C#'s margin and top-left anchors.
    pub fn with_label(mut label: Box<dyn Widget>) -> Self {
        // C#: Padding = new BorderDouble(14, 14, 14, 16) (left, bottom, right, top).
        let padding = Insets::from_sides(14.0, 14.0, 16.0, 14.0);
        if let Some(base) = label.widget_base_mut() {
            // C#: new BorderDouble(20, 0, 0, -Padding.Top).
            base.margin = Insets::from_sides(20.0, 0.0, -padding.top, 0.0);
            base.v_anchor = VAnchor::TOP;
            base.h_anchor = HAnchor::LEFT;
        }
        let client_area = AbsoluteLayout::new()
            .with_h_anchor(HAnchor::MAX_FIT_OR_STRETCH)
            .with_v_anchor(VAnchor::MAX_FIT_OR_STRETCH);
        Self {
            bounds: Rect::default(),
            children: vec![label, Box::new(client_area)],
            base: WidgetBase::new()
                .with_h_anchor(HAnchor::FIT)
                .with_v_anchor(VAnchor::FIT),
            padding,
            local_low: Point::ORIGIN,
            border_color: Color::black(),
        }
    }

    /// Add content to the client area (C#'s `AddChild` override).
    // Named like `AbsoluteLayout::add`, the crate's builder idiom.
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, child: Box<dyn Widget>) -> Self {
        self.children[CLIENT_AREA].children_mut().push(child);
        self
    }

    /// The area content goes into (C# `ClientArea`).
    pub fn client_area(&self) -> &AbsoluteLayout {
        self.children[CLIENT_AREA]
            .as_any()
            .and_then(|a| a.downcast_ref::<AbsoluteLayout>())
            .expect("GroupBox's client area is an AbsoluteLayout")
    }

    /// The area content goes into, mutably (C# `ClientArea`).
    pub fn client_area_mut(&mut self) -> &mut AbsoluteLayout {
        self.children[CLIENT_AREA]
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<AbsoluteLayout>())
            .expect("GroupBox's client area is an AbsoluteLayout")
    }

    /// The colour of the frame (C# `BorderColor`, black by default).
    pub fn with_border_color(mut self, color: Color) -> Self {
        self.border_color = color;
        self
    }

    /// C#'s `LocalBounds` (see [`AbsoluteLayout::local_bounds`]).
    pub fn local_bounds(&self) -> Rect {
        Rect::new(
            self.local_low.x,
            self.local_low.y,
            self.bounds.width,
            self.bounds.height,
        )
    }
}

impl Widget for GroupBox {
    fn type_name(&self) -> &'static str {
        "GroupBox"
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
        super::simple_align::layout_simple_align(
            &mut self.children,
            &self.base,
            self.padding,
            &mut self.local_low,
            available,
        )
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let scale = crate::device_scale();
        let inset = 8.5 * scale;
        let (w, h) = (self.bounds.width, self.bounds.height);
        let label = self.children[0].bounds();
        let (l, r, b, t) = (inset, w - inset, inset, h - inset);
        ctx.set_stroke_color(self.border_color);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        // bottom, left and right
        ctx.move_to(l, t);
        ctx.line_to(l, b);
        ctx.line_to(r, b);
        ctx.line_to(r, t);
        // top, broken around the label
        ctx.line_to(label.x + label.width + 2.0 * scale, t);
        ctx.move_to(label.x - 2.0 * scale, t);
        ctx.line_to(l, t);
        ctx.stroke();
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
