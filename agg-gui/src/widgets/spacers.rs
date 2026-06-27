//! Spacing primitives: `Spacer` (flex filler) and `Separator` (divider line).
//!
//! Split out of `primitives.rs` so each widget module stays under the project's
//! 800-line file-size limit; behaviour is unchanged.

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

// ---------------------------------------------------------------------------
// Spacer — flexible empty space for use in flex layouts
// ---------------------------------------------------------------------------

/// An invisible leaf widget that expands to fill available space.
///
/// Used as a `flex` child in [`FlexColumn`][crate::FlexColumn] or
/// [`FlexRow`][crate::FlexRow] to push siblings apart.
pub struct Spacer {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
}

impl Spacer {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
        }
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
}

impl Default for Spacer {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Spacer {
    fn type_name(&self) -> &'static str {
        "Spacer"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
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

    fn layout(&mut self, available: Size) -> Size {
        available
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// Separator — a thin horizontal or vertical divider line
// ---------------------------------------------------------------------------

/// A thin horizontal or vertical divider line.
///
/// When no explicit colour is set via [`with_color`](Separator::with_color),
/// the separator reads its colour from the active theme's `separator` field at
/// paint time, so it automatically adapts to dark / light mode.
pub struct Separator {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    vertical: bool,
    line_inset: f64,
    /// `None` → use `ctx.visuals().separator` at paint time.
    color: Option<Color>,
}

impl Separator {
    /// Create a horizontal separator (the common case).
    pub fn horizontal() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            vertical: false,
            line_inset: 4.0,
            color: None,
        }
    }

    /// Create a vertical separator.
    pub fn vertical() -> Self {
        Self {
            vertical: true,
            ..Self::horizontal()
        }
    }

    pub fn with_line_inset(mut self, m: f64) -> Self {
        self.line_inset = m;
        self
    }
    pub fn with_color(mut self, c: Color) -> Self {
        self.color = Some(c);
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
}

impl Widget for Separator {
    fn type_name(&self) -> &'static str {
        "Separator"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
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

    fn layout(&mut self, available: Size) -> Size {
        if self.vertical {
            Size::new(1.0 + self.line_inset * 2.0, available.height)
        } else {
            Size::new(available.width, 1.0 + self.line_inset * 2.0)
        }
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let color = self.color.unwrap_or_else(|| ctx.visuals().separator);
        ctx.set_fill_color(color);
        ctx.begin_path();
        if self.vertical {
            ctx.rect(self.line_inset, 0.0, 1.0, h);
        } else {
            ctx.rect(0.0, self.line_inset, w, 1.0);
        }
        ctx.fill();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
