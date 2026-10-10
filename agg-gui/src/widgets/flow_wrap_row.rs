//! One row of a [`FlowLeftRightWithWrapping`](super::flow_left_right_with_wrapping):
//! C#'s row `FlowLayoutWidget` (left to right, `HAnchor.Stretch`, the flow's
//! `RowMargin` / `RowPadding`, and its `RowBorder` / `RowBorderColor` on every
//! row after the first).
//!
//! Items sit left to right from the row's inner left edge (inside its border
//! and padding), each at its natural width plus its margin; items that stretch
//! share whatever width the fixed ones leave, never less than their minimum.
//! The row is as tall as its tallest item with its margins, plus padding and
//! border, and items sit on its inner bottom edge.

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets};
use crate::widget::Widget;

/// A row of a wrapping flow.
pub struct WrapRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    margin: Insets,
    padding: Insets,
    border: Insets,
    border_color: Color,
}

fn stretches(child: &dyn Widget) -> bool {
    child.h_anchor() == HAnchor::STRETCH
}

impl WrapRow {
    pub(crate) fn new(
        margin: Insets,
        padding: Insets,
        border: Insets,
        border_color: Color,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            margin,
            padding,
            border,
            border_color,
        }
    }

    /// C# `Padding`.
    pub fn padding(&self) -> Insets {
        self.padding
    }

    /// C# `Border`.
    pub fn border(&self) -> Insets {
        self.border
    }

    pub(crate) fn push(&mut self, child: Box<dyn Widget>) {
        self.children.push(child);
    }

    pub(crate) fn insert(&mut self, index: usize, child: Box<dyn Widget>) {
        let index = index.min(self.children.len());
        self.children.insert(index, child);
    }

    /// C# `GetChildrenBoundsIncludingMargins().Width`: the children's widths
    /// and margins end to end, a stretching child at its minimum.
    pub(crate) fn content_width(&mut self) -> f64 {
        self.children
            .iter_mut()
            .map(|child| {
                let width = if stretches(child.as_ref()) {
                    child.min_size().width
                } else {
                    child.layout(Size::new(f64::MAX, f64::MAX)).width
                };
                let m = child.margin();
                width + m.left + m.right
            })
            .sum()
    }
}

impl Widget for WrapRow {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "WrapRow"
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
        self.margin
    }
    fn h_anchor(&self) -> HAnchor {
        HAnchor::STRETCH
    }

    fn layout(&mut self, available: Size) -> Size {
        let (b, p) = (self.border, self.padding);
        let inner_w = (available.width - b.left - b.right - p.left - p.right).max(0.0);
        // Fixed items first, at their natural widths.
        let mut widths = vec![0.0; self.children.len()];
        let mut heights = vec![0.0; self.children.len()];
        let mut fixed = 0.0;
        let mut stretch_count = 0;
        for (i, child) in self.children.iter_mut().enumerate() {
            let m = child.margin();
            if stretches(child.as_ref()) {
                stretch_count += 1;
            } else {
                let size = child.layout(Size::new(inner_w, f64::MAX));
                widths[i] = size.width;
                heights[i] = size.height;
            }
            fixed += widths[i] + m.left + m.right;
        }
        // Stretching items share the rest.
        if stretch_count > 0 {
            let share = ((inner_w - fixed) / stretch_count as f64).max(0.0);
            for (i, child) in self.children.iter_mut().enumerate() {
                if stretches(child.as_ref()) {
                    let width = share.max(child.min_size().width);
                    let size = child.layout(Size::new(width, f64::MAX));
                    widths[i] = width;
                    heights[i] = size.height;
                }
            }
        }
        let tallest = self
            .children
            .iter()
            .zip(&heights)
            .map(|(child, h)| {
                let m = child.margin();
                h + m.top + m.bottom
            })
            .fold(0.0, f64::max);
        let mut x = b.left + p.left;
        for (i, child) in self.children.iter_mut().enumerate() {
            let m = child.margin();
            x += m.left;
            child.set_bounds(Rect::new(
                x,
                b.bottom + p.bottom + m.bottom,
                widths[i],
                heights[i],
            ));
            x += widths[i] + m.right;
        }
        Size::new(
            available.width,
            tallest + b.top + b.bottom + p.top + p.bottom,
        )
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let (b, w, h) = (self.border, self.bounds.width, self.bounds.height);
        if b == Insets::ZERO {
            return;
        }
        ctx.set_fill_color(self.border_color);
        for (x, y, bw, bh) in [
            (0.0, 0.0, b.left, h),
            (w - b.right, 0.0, b.right, h),
            (0.0, h - b.top, w, b.top),
            (0.0, 0.0, w, b.bottom),
        ] {
            if bw > 0.0 && bh > 0.0 {
                ctx.begin_path();
                ctx.rect(x, y, bw, bh);
                ctx.fill();
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
