//! Primitive layout widgets: Stack, Padding, SizedBox, Spacer, Separator.

use crate::color::Color;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::widget::Widget;

// ---------------------------------------------------------------------------
// Stack — overlays children at the same position (first = back, last = front)
// ---------------------------------------------------------------------------

/// Stacks children on top of each other, each sized to fill the stack's area.
///
/// Paint order: first child is drawn first (furthest back). The last child
/// appears on top. Hit testing also follows paint order (reverse).
pub struct Stack {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Stack {
    pub fn new() -> Self {
        Self { bounds: Rect::default(), children: Vec::new() }
    }

    pub fn add(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self
    }
}

impl Default for Stack { fn default() -> Self { Self::new() } }

impl Widget for Stack {
    fn type_name(&self) -> &'static str { "Stack" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        for child in &mut self.children {
            child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, available.width, available.height));
        }
        available
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// Padding — wraps one child with uniform insets
// ---------------------------------------------------------------------------

/// Surrounds a single child with uniform padding.
pub struct Padding {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    amount: f64,
}

impl Padding {
    pub fn new(amount: f64, child: Box<dyn Widget>) -> Self {
        Self { bounds: Rect::default(), children: vec![child], amount }
    }
}

impl Widget for Padding {
    fn type_name(&self) -> &'static str { "Padding" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let p = self.amount;
        let inner = Size::new((available.width - p * 2.0).max(0.0), (available.height - p * 2.0).max(0.0));
        if let Some(child) = self.children.first_mut() {
            let desired = child.layout(inner);
            child.set_bounds(Rect::new(p, p, desired.width, desired.height));
        }
        Size::new(
            self.children.first().map_or(0.0, |c| c.bounds().width) + p * 2.0,
            self.children.first().map_or(0.0, |c| c.bounds().height) + p * 2.0,
        )
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// SizedBox — forces specific width and/or height
// ---------------------------------------------------------------------------

/// Forces a specific size on its optional child.
///
/// If `width` or `height` is `None`, the available size on that axis is passed
/// through unchanged.
pub struct SizedBox {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

impl SizedBox {
    pub fn new() -> Self {
        Self { bounds: Rect::default(), children: Vec::new(), width: None, height: None }
    }

    pub fn with_width(mut self, w: f64) -> Self { self.width = Some(w); self }
    pub fn with_height(mut self, h: f64) -> Self { self.height = Some(h); self }

    pub fn with_child(mut self, child: Box<dyn Widget>) -> Self {
        self.children.clear();
        self.children.push(child);
        self
    }

    /// Create a fixed-size empty box (gap / spacer with exact dimensions).
    pub fn fixed(width: f64, height: f64) -> Self {
        Self::new().with_width(width).with_height(height)
    }
}

impl Default for SizedBox { fn default() -> Self { Self::new() } }

impl Widget for SizedBox {
    fn type_name(&self) -> &'static str { "SizedBox" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let w = self.width.unwrap_or(available.width);
        let h = self.height.unwrap_or(available.height);
        if let Some(child) = self.children.first_mut() {
            child.layout(Size::new(w, h));
            child.set_bounds(Rect::new(0.0, 0.0, w, h));
        }
        Size::new(w, h)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

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
}

impl Spacer {
    pub fn new() -> Self {
        Self { bounds: Rect::default(), children: Vec::new() }
    }
}

impl Default for Spacer { fn default() -> Self { Self::new() } }

impl Widget for Spacer {
    fn type_name(&self) -> &'static str { "Spacer" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size { available }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// Separator — a thin horizontal or vertical divider line
// ---------------------------------------------------------------------------

/// A thin horizontal or vertical divider line.
pub struct Separator {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    vertical: bool,
    margin: f64,
    color: Color,
}

impl Separator {
    /// Create a horizontal separator (the common case).
    pub fn horizontal() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            vertical: false,
            margin: 4.0,
            color: Color::rgba(0.0, 0.0, 0.0, 0.12),
        }
    }

    /// Create a vertical separator.
    pub fn vertical() -> Self {
        Self { vertical: true, ..Self::horizontal() }
    }

    pub fn with_margin(mut self, m: f64) -> Self { self.margin = m; self }
    pub fn with_color(mut self, c: Color) -> Self { self.color = c; self }
}

impl Widget for Separator {
    fn type_name(&self) -> &'static str { "Separator" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        if self.vertical {
            Size::new(1.0 + self.margin * 2.0, available.height)
        } else {
            Size::new(available.width, 1.0 + self.margin * 2.0)
        }
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        ctx.set_fill_color(self.color);
        ctx.begin_path();
        if self.vertical {
            ctx.rect(self.margin, 0.0, 1.0, h);
        } else {
            ctx.rect(0.0, self.margin, w, 1.0);
        }
        ctx.fill();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}
