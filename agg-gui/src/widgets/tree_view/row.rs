//! Compositional row widgets for `TreeView`:
//! `ExpandToggle`, `NodeIconWidget`, and `TreeRow`.
//!
//! These widgets are intended to be composed into a `FlexRow` (or positioned
//! manually) by the `TreeView` when building visible rows.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::label::Label;
use crate::widgets::primitives::SizedBox;

use super::node::NodeIcon;

// ---------------------------------------------------------------------------
// Constants (moved from mod.rs so drag.rs and row.rs share one source)
// ---------------------------------------------------------------------------

pub const EXPAND_W: f64 = 18.0; // space reserved for expand arrow
pub const ICON_W:   f64 = 14.0;
pub const ICON_GAP: f64 = 4.0;

// ---------------------------------------------------------------------------
// icon_color helper
// ---------------------------------------------------------------------------

/// Return the fill colour for a given node icon type.
pub fn icon_color(icon: NodeIcon) -> Color {
    match icon {
        NodeIcon::Folder  => Color::rgb(0.90, 0.72, 0.20),
        NodeIcon::File    => Color::rgb(0.55, 0.78, 0.95),
        NodeIcon::Package => Color::rgb(0.70, 0.60, 0.88),
    }
}

// ---------------------------------------------------------------------------
// ExpandToggle
// ---------------------------------------------------------------------------

/// A fixed-width cell that draws an expand/collapse triangle when the node
/// `has_children`.  Width is always `EXPAND_W`; height fills the row.
pub struct ExpandToggle {
    bounds: Rect,
    pub has_children: bool,
    pub is_expanded: bool,
    children: Vec<Box<dyn Widget>>,
}

impl ExpandToggle {
    pub fn new(has_children: bool, is_expanded: bool) -> Self {
        Self {
            bounds: Rect::default(),
            has_children,
            is_expanded,
            children: Vec::new(),
        }
    }
}

impl Widget for ExpandToggle {
    fn type_name(&self) -> &'static str { "ExpandToggle" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        Size::new(EXPAND_W, available.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.has_children { return; }

        let w = self.bounds.width;
        let h = self.bounds.height;
        let cx = w * 0.5;
        let cy = h * 0.5;

        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.45));
        ctx.begin_path();
        if self.is_expanded {
            // Down-pointing ▼
            ctx.move_to(cx - 4.5, cy + 2.0);
            ctx.line_to(cx + 4.5, cy + 2.0);
            ctx.line_to(cx, cy - 3.0);
            ctx.close_path();
        } else {
            // Right-pointing ▶
            ctx.move_to(cx - 2.5, cy - 4.5);
            ctx.line_to(cx - 2.5, cy + 4.5);
            ctx.line_to(cx + 3.5, cy);
            ctx.close_path();
        }
        ctx.fill();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// NodeIconWidget
// ---------------------------------------------------------------------------

/// Draws the coloured icon glyph for a node.
/// Width is `ICON_W + ICON_GAP`; height fills the row.
pub struct NodeIconWidget {
    bounds: Rect,
    pub icon: NodeIcon,
    children: Vec<Box<dyn Widget>>,
}

impl NodeIconWidget {
    pub fn new(icon: NodeIcon) -> Self {
        Self {
            bounds: Rect::default(),
            icon,
            children: Vec::new(),
        }
    }
}

impl Widget for NodeIconWidget {
    fn type_name(&self) -> &'static str { "NodeIconWidget" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        Size::new(ICON_W + ICON_GAP, available.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let h = self.bounds.height;
        let iy = (h - ICON_W) * 0.5;

        ctx.set_fill_color(icon_color(self.icon));
        ctx.begin_path();
        ctx.rounded_rect(0.0, iy, ICON_W, ICON_W, 2.0);
        ctx.fill();

        if matches!(self.icon, NodeIcon::Folder) {
            // Folder tab nub
            ctx.begin_path();
            ctx.rounded_rect(0.0, iy + ICON_W * 0.55, ICON_W * 0.45, ICON_W * 0.5, 1.0);
            ctx.fill();
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// TreeRow
// ---------------------------------------------------------------------------

/// A single visible row in the tree, composed of:
///   [0] SizedBox      — indentation spacer
///   [1] ExpandToggle  — expand/collapse arrow
///   [2] NodeIconWidget — coloured icon
///   [3] Label         — node label text
pub struct TreeRow {
    bounds: Rect,
    pub node_idx: usize,
    /// Bounds of the ExpandToggle in row-local coordinates (X offset, full height).
    /// Cached during `layout` so the `TreeView` can do hit-testing for toggle clicks.
    pub toggle_local_bounds: Rect,
    is_selected: bool,
    is_hovered: bool,
    focused: bool,
    children: Vec<Box<dyn Widget>>,
}

impl TreeRow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        node_idx: usize,
        depth: u32,
        has_children: bool,
        is_expanded: bool,
        is_selected: bool,
        is_hovered: bool,
        focused: bool,
        icon: NodeIcon,
        label: impl Into<String>,
        font: Arc<Font>,
        font_size: f64,
        indent_width: f64,
        row_height: f64,
    ) -> Self {
        let indent_px = depth as f64 * indent_width;
        let mut children: Vec<Box<dyn Widget>> = Vec::with_capacity(4);
        children.push(Box::new(SizedBox::fixed(indent_px, row_height)));
        children.push(Box::new(ExpandToggle::new(has_children, is_expanded)));
        children.push(Box::new(NodeIconWidget::new(icon)));
        children.push(Box::new(Label::new(label, font).with_font_size(font_size)));

        Self {
            bounds: Rect::default(),
            node_idx,
            toggle_local_bounds: Rect::default(),
            is_selected,
            is_hovered,
            focused,
            children,
        }
    }
}

impl Widget for TreeRow {
    fn type_name(&self) -> &'static str { "TreeRow" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let h = available.height;
        let total_w = available.width;

        // Children 0, 1, 2 get their natural width.
        // Child 3 (Label) gets the remaining width.
        let mut x = 0.0;

        // Child 0: SizedBox (indent)
        let s0 = self.children[0].layout(Size::new(total_w, h));
        self.children[0].set_bounds(Rect::new(x, 0.0, s0.width, h));
        x += s0.width;

        // Child 1: ExpandToggle — cache its x for toggle hit-testing
        let s1 = self.children[1].layout(Size::new(total_w - x, h));
        self.children[1].set_bounds(Rect::new(x, 0.0, s1.width, h));
        self.toggle_local_bounds = Rect::new(x, 0.0, s1.width, h);
        x += s1.width;

        // Child 2: NodeIconWidget
        let s2 = self.children[2].layout(Size::new(total_w - x, h));
        self.children[2].set_bounds(Rect::new(x, 0.0, s2.width, h));
        x += s2.width;

        // Child 3: Label — remaining width
        let label_w = (total_w - x).max(0.0);
        let s3 = self.children[3].layout(Size::new(label_w, h));
        self.children[3].set_bounds(Rect::new(x, 0.0, s3.width, h));

        Size::new(total_w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;

        if self.is_selected {
            let c = if self.focused {
                Color::rgba(0.22, 0.45, 0.88, 0.15)
            } else {
                Color::rgba(0.0, 0.0, 0.0, 0.07)
            };
            ctx.set_fill_color(c);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, h);
            ctx.fill();
        } else if self.is_hovered {
            ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.04));
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, h);
            ctx.fill();
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}
