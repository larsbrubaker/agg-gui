//! Primitive layout widgets: Stack, Padding, SizedBox, Spacer, Separator.

use crate::color::Color;
use crate::device_scale::device_scale;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase, resolve_fit_or_stretch};
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
    base: WidgetBase,
}

impl Stack {
    pub fn new() -> Self {
        Self { bounds: Rect::default(), children: Vec::new(), base: WidgetBase::new() }
    }

    pub fn add(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }
}

impl Default for Stack { fn default() -> Self { Self::new() } }

impl Widget for Stack {
    fn type_name(&self) -> &'static str { "Stack" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn layout(&mut self, available: Size) -> Size {
        for child in &mut self.children {
            child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, available.width, available.height));
        }

        // Bring-to-front pass — **after** children.layout on purpose.
        //
        // A raise can be requested from two places:
        //   1. Widget input handlers (e.g. `Window::on_event` firing on a
        //      MouseDown inside the window — "click to raise").  These run
        //      BEFORE the frame's layout pass, so the flag is already set
        //      by the time we get here.
        //   2. Widget `layout()` itself (e.g. `Window` detects the
        //      `visible_cell` false→true rising edge at layout time, so
        //      toggling a demo on from the sidebar raises its window).
        //      These set the flag DURING this very layout pass.
        //
        // Draining the flags AFTER children.layout catches both cases in
        // the same frame — no one-frame visual delay.  The reactive-mode
        // event loop only renders once per event, so a one-frame delay
        // means the raise is invisible until the next unrelated event
        // arrives, which is what the user reported (sidebar-opened windows
        // appearing in the back).
        let mut i = 0;
        let mut raised: Vec<Box<dyn Widget>> = Vec::new();
        while i < self.children.len() {
            if self.children[i].take_raise_request() {
                raised.push(self.children.remove(i));
                // Don't advance `i` — the list just shortened.
            } else {
                i += 1;
            }
        }
        for r in raised {
            self.children.push(r);
        }

        available
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// Padding — wraps one child with per-side insets
// ---------------------------------------------------------------------------

/// Surrounds a single child with configurable per-side padding.
pub struct Padding {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    insets: Insets,
}

impl Padding {
    /// Explicit per-side padding.
    pub fn new(insets: Insets, child: Box<dyn Widget>) -> Self {
        Self { bounds: Rect::default(), children: vec![child], base: WidgetBase::new(), insets }
    }

    /// Uniform padding on all four sides.
    pub fn uniform(amount: f64, child: Box<dyn Widget>) -> Self {
        Self::new(Insets::all(amount), child)
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }
}

impl Widget for Padding {
    fn type_name(&self) -> &'static str { "Padding" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn layout(&mut self, available: Size) -> Size {
        let p = &self.insets;
        let inner = Size::new(
            (available.width  - p.left - p.right ).max(0.0),
            (available.height - p.top  - p.bottom).max(0.0),
        );
        if let Some(child) = self.children.first_mut() {
            let desired = child.layout(inner);
            // In Y-up coordinates: origin of the child content is at (left, bottom).
            child.set_bounds(Rect::new(p.left, p.bottom, desired.width, desired.height));
        }
        // Report total size including insets.
        let content_w = self.children.first().map_or(0.0, |c| c.bounds().width);
        let content_h = self.children.first().map_or(0.0, |c| c.bounds().height);
        Size::new(content_w + p.left + p.right, content_h + p.top + p.bottom)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// SizedBox — forces specific width and/or height, with anchor-aware child placement
// ---------------------------------------------------------------------------

/// Forces a specific size on its optional child.
///
/// If `width` or `height` is `None`, the available size on that axis is passed
/// through unchanged.  The child is placed within the box using its own
/// `h_anchor` and `v_anchor`, respecting its `margin`.
pub struct SizedBox {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

impl SizedBox {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            width: None,
            height: None,
        }
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

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }
}

impl Default for SizedBox { fn default() -> Self { Self::new() } }

impl Widget for SizedBox {
    fn type_name(&self) -> &'static str { "SizedBox" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn layout(&mut self, available: Size) -> Size {
        // Fall back to the available axis only for dimensions that haven't been
        // explicitly set AND don't have a child to size to; otherwise use the
        // child's natural size on that axis so the SizedBox reports a sensible
        // height when only a width was supplied (e.g. narrow DragValue wrapper).
        //
        // When neither height nor child is present (pure horizontal spacer,
        // e.g. `SizedBox::new().with_width(8.0)`), default the height to zero
        // so the spacer doesn't inflate the parent row/column to the full
        // available axis — which would otherwise push sibling widgets off
        // screen.
        let w = self.width.unwrap_or(available.width);
        let mut h = self.height.unwrap_or_else(|| {
            if self.children.is_empty() { 0.0 } else { available.height }
        });

        if let Some(child) = self.children.first_mut() {
            let scale  = device_scale();
            let m      = child.margin().scale(scale);
            let slot_w = (w - m.left - m.right ).max(0.0);
            let slot_h = (h - m.top  - m.bottom).max(0.0);

            let desired = child.layout(Size::new(slot_w, slot_h));

            // If the caller didn't pin the height, shrink to the child's
            // natural height plus its vertical margin.
            if self.height.is_none() {
                h = (desired.height + m.vertical())
                    .clamp(self.base.min_size.height, self.base.max_size.height);
            }

            // Horizontal placement within the box (margin already limits slot).
            let h_anchor = child.h_anchor();
            let min_w    = child.min_size().width;
            let max_w    = child.max_size().width;
            let child_w  = if h_anchor.is_stretch() {
                slot_w.clamp(min_w, max_w)
            } else if h_anchor == HAnchor::MAX_FIT_OR_STRETCH {
                resolve_fit_or_stretch(desired.width, slot_w, true).clamp(min_w, max_w)
            } else if h_anchor == HAnchor::MIN_FIT_OR_STRETCH {
                resolve_fit_or_stretch(desired.width, slot_w, false).clamp(min_w, max_w)
            } else {
                desired.width.clamp(min_w, max_w)
            };

            let child_x = if h_anchor.contains(HAnchor::RIGHT) && !h_anchor.contains(HAnchor::LEFT) {
                (w - m.right - child_w).max(0.0)
            } else if h_anchor.contains(HAnchor::CENTER) && !h_anchor.is_stretch() {
                m.left + (slot_w - child_w) * 0.5
            } else {
                m.left
            };

            // Vertical placement (Y-up: BOTTOM = low Y).
            let v_anchor = child.v_anchor();
            let min_h    = child.min_size().height;
            let max_h    = child.max_size().height;
            let child_h  = if v_anchor.is_stretch() {
                slot_h.clamp(min_h, max_h)
            } else if v_anchor == VAnchor::MAX_FIT_OR_STRETCH {
                resolve_fit_or_stretch(desired.height, slot_h, true).clamp(min_h, max_h)
            } else if v_anchor == VAnchor::MIN_FIT_OR_STRETCH {
                resolve_fit_or_stretch(desired.height, slot_h, false).clamp(min_h, max_h)
            } else {
                desired.height.clamp(min_h, max_h)
            };

            let child_y = if v_anchor.contains(VAnchor::TOP) && !v_anchor.contains(VAnchor::BOTTOM) {
                (h - m.top - child_h).max(0.0)
            } else if v_anchor.contains(VAnchor::CENTER) && !v_anchor.is_stretch() {
                m.bottom + (slot_h - child_h) * 0.5
            } else {
                m.bottom
            };

            child.set_bounds(Rect::new(child_x, child_y, child_w, child_h));
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
    base: WidgetBase,
}

impl Spacer {
    pub fn new() -> Self {
        Self { bounds: Rect::default(), children: Vec::new(), base: WidgetBase::new() }
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }
}

impl Default for Spacer { fn default() -> Self { Self::new() } }

impl Widget for Spacer {
    fn type_name(&self) -> &'static str { "Spacer" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn layout(&mut self, available: Size) -> Size { available }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
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
        Self { vertical: true, ..Self::horizontal() }
    }

    pub fn with_line_inset(mut self, m: f64) -> Self { self.line_inset = m; self }
    pub fn with_color(mut self, c: Color) -> Self { self.color = Some(c); self }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }
}

impl Widget for Separator {
    fn type_name(&self) -> &'static str { "Separator" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

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

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}
