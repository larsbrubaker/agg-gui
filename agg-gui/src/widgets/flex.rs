//! Flex layout widgets: `FlexColumn` (vertical) and `FlexRow` (horizontal).
//!
//! # Y-up layout convention
//!
//! `FlexColumn` stacks children **top to bottom** visually, which in Y-up
//! coordinates means the *first* child gets the *highest* Y values. The layout
//! cursor starts at the top of the available area and moves downward.
//!
//! `FlexRow` stacks children **left to right**, as expected.
//!
//! # Flex algorithm
//!
//! Each child has a `flex` factor (stored in a parallel `Vec<f64>`):
//! - `flex = 0.0` → "fixed": the child is laid out at its natural size on
//!   the main axis.
//! - `flex > 0.0` → "growing": the child receives a proportional share of
//!   the remaining space after all fixed children are measured.
//!
//! Children with equal `flex` values split remaining space equally.

use crate::color::Color;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::widget::Widget;

// ---------------------------------------------------------------------------
// FlexColumn
// ---------------------------------------------------------------------------

/// Stacks children top-to-bottom (first child = visually topmost).
pub struct FlexColumn {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    /// Parallel to `children`. 0.0 = fixed; >0 = flex fraction.
    flex_factors: Vec<f64>,
    pub gap: f64,
    pub padding: f64,
    pub background: Color,
}

impl FlexColumn {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            flex_factors: Vec::new(),
            gap: 0.0,
            padding: 0.0,
            background: Color::rgba(0.0, 0.0, 0.0, 0.0),
        }
    }

    pub fn with_gap(mut self, gap: f64) -> Self { self.gap = gap; self }
    pub fn with_padding(mut self, p: f64) -> Self { self.padding = p; self }
    pub fn with_background(mut self, c: Color) -> Self { self.background = c; self }

    /// Add a fixed-size child (flex = 0).
    pub fn add(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self.flex_factors.push(0.0);
        self
    }

    /// Add a flex child that expands proportionally.
    pub fn add_flex(mut self, child: Box<dyn Widget>, flex: f64) -> Self {
        self.children.push(child);
        self.flex_factors.push(flex.max(0.0));
        self
    }

    /// Push a child directly (for use with `children_mut()`).
    pub fn push(&mut self, child: Box<dyn Widget>, flex: f64) {
        self.children.push(child);
        self.flex_factors.push(flex.max(0.0));
    }
}

impl Default for FlexColumn { fn default() -> Self { Self::new() } }

impl Widget for FlexColumn {
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let pad = self.padding;
        let gap = self.gap;
        let n = self.children.len();
        if n == 0 { return available; }

        let inner_w = (available.width - pad * 2.0).max(0.0);
        let inner_h = (available.height - pad * 2.0).max(0.0);
        let total_gap = if n > 1 { gap * (n - 1) as f64 } else { 0.0 };

        // Step 1: measure fixed children.
        let mut fixed_heights = vec![0.0f64; n];
        let mut total_fixed = 0.0f64;
        let mut total_flex = 0.0f64;
        for i in 0..n {
            if self.flex_factors[i] == 0.0 {
                // Give fixed children the full inner_w; height = inf for natural measure.
                let desired = self.children[i].layout(Size::new(inner_w, inner_h));
                fixed_heights[i] = desired.height;
                total_fixed += desired.height;
            } else {
                total_flex += self.flex_factors[i];
            }
        }

        // Step 2: distribute remaining space to flex children.
        let remaining = (inner_h - total_fixed - total_gap).max(0.0);
        let flex_unit = if total_flex > 0.0 { remaining / total_flex } else { 0.0 };

        // Step 3: assign heights and lay out all children.
        let mut assigned_heights = vec![0.0f64; n];
        for i in 0..n {
            assigned_heights[i] = if self.flex_factors[i] == 0.0 {
                fixed_heights[i]
            } else {
                self.flex_factors[i] * flex_unit
            };
        }

        // Natural content height: the actual extent of all children + gaps.
        // When there are no flex children this fully determines the column's
        // size regardless of how much space the parent offered.  When flex
        // children are present the parent-supplied inner_h is used so they
        // can expand to fill.  This avoids placing children at astronomically
        // large Y coordinates when a ScrollView passes f64::MAX / 2.0.
        let natural_content_h = total_fixed + total_gap;
        let effective_h = if total_flex > 0.0 { inner_h } else { natural_content_h };

        // Step 4: place children top-to-bottom in Y-up.
        let mut cursor_y = pad + effective_h;
        for i in 0..n {
            let ch = assigned_heights[i];
            let child_y = cursor_y - ch; // bottom-left of this child
            let desired = self.children[i].layout(Size::new(inner_w, ch));
            let actual_w = desired.width.min(inner_w);
            self.children[i].set_bounds(Rect::new(pad, child_y, actual_w, ch));
            cursor_y = child_y - gap;
        }

        // Return natural size for all-fixed layouts.  This lets ScrollView
        // read the true content_height from layout()'s return value.
        if total_flex > 0.0 {
            available
        } else {
            Size::new(available.width, natural_content_h + pad * 2.0)
        }
    }

    fn paint(&mut self, ctx: &mut GfxCtx) {
        if self.background.a > 0.001 {
            let w = self.bounds.width;
            let h = self.bounds.height;
            ctx.set_fill_color(self.background);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, h);
            ctx.fill();
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// FlexRow
// ---------------------------------------------------------------------------

/// Arranges children left-to-right (first child = leftmost).
pub struct FlexRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    flex_factors: Vec<f64>,
    pub gap: f64,
    pub padding: f64,
    pub background: Color,
}

impl FlexRow {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            flex_factors: Vec::new(),
            gap: 0.0,
            padding: 0.0,
            background: Color::rgba(0.0, 0.0, 0.0, 0.0),
        }
    }

    pub fn with_gap(mut self, gap: f64) -> Self { self.gap = gap; self }
    pub fn with_padding(mut self, p: f64) -> Self { self.padding = p; self }
    pub fn with_background(mut self, c: Color) -> Self { self.background = c; self }

    pub fn add(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self.flex_factors.push(0.0);
        self
    }

    pub fn add_flex(mut self, child: Box<dyn Widget>, flex: f64) -> Self {
        self.children.push(child);
        self.flex_factors.push(flex.max(0.0));
        self
    }

    pub fn push(&mut self, child: Box<dyn Widget>, flex: f64) {
        self.children.push(child);
        self.flex_factors.push(flex.max(0.0));
    }
}

impl Default for FlexRow { fn default() -> Self { Self::new() } }

impl Widget for FlexRow {
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let pad = self.padding;
        let gap = self.gap;
        let n = self.children.len();
        if n == 0 { return available; }

        let inner_w = (available.width - pad * 2.0).max(0.0);
        let inner_h = (available.height - pad * 2.0).max(0.0);
        let total_gap = if n > 1 { gap * (n - 1) as f64 } else { 0.0 };

        // Measure fixed children.
        let mut fixed_widths = vec![0.0f64; n];
        let mut total_fixed = 0.0f64;
        let mut total_flex = 0.0f64;
        for i in 0..n {
            if self.flex_factors[i] == 0.0 {
                let desired = self.children[i].layout(Size::new(inner_w, inner_h));
                fixed_widths[i] = desired.width;
                total_fixed += desired.width;
            } else {
                total_flex += self.flex_factors[i];
            }
        }

        let remaining = (inner_w - total_fixed - total_gap).max(0.0);
        let flex_unit = if total_flex > 0.0 { remaining / total_flex } else { 0.0 };

        // Assign widths and lay out left-to-right.
        let mut cursor_x = pad;
        for i in 0..n {
            let cw = if self.flex_factors[i] == 0.0 {
                fixed_widths[i]
            } else {
                self.flex_factors[i] * flex_unit
            };
            let desired = self.children[i].layout(Size::new(cw, inner_h));
            let actual_h = desired.height.min(inner_h);
            // Align to the bottom of the row (y = pad).
            self.children[i].set_bounds(Rect::new(cursor_x, pad, cw, actual_h));
            cursor_x += cw + gap;
        }

        // Return the natural (intrinsic) height: tallest child + vertical padding.
        // Returning `available` would propagate a huge height (e.g. f64::MAX/2 from
        // ScrollView) when this FlexRow is a fixed child of a FlexColumn, causing
        // the FlexColumn to place all sibling widgets at near-zero or negative Y and
        // making the scroll content appear astronomically tall — which in turn gives
        // AGG coordinates near ±4.5e15, overflowing its rasterizer.
        let max_child_h = self.children.iter()
            .map(|c| c.bounds().height)
            .fold(0.0_f64, f64::max);
        let natural_h = max_child_h + pad * 2.0;
        Size::new(available.width, natural_h)
    }

    fn paint(&mut self, ctx: &mut GfxCtx) {
        if self.background.a > 0.001 {
            let w = self.bounds.width;
            let h = self.bounds.height;
            ctx.set_fill_color(self.background);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, h);
            ctx.fill();
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}
