//! Compositional row widgets for `TreeView`:
//! `ExpandToggle`, `NodeIconWidget`, and `TreeRow`.
//!
//! These widgets are intended to be composed into a `FlexRow` (or positioned
//! manually) by the `TreeView` when building visible rows.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::label::Label;
use crate::widgets::primitives::SizedBox;

use super::node::NodeIcon;

// ---------------------------------------------------------------------------
// Constants (moved from mod.rs so drag.rs and row.rs share one source)
// ---------------------------------------------------------------------------

pub const EXPAND_W: f64 = 18.0; // space reserved for expand arrow
pub const ICON_W: f64 = 14.0;
pub const ICON_GAP: f64 = 4.0;

// ---------------------------------------------------------------------------
// icon_color helper
// ---------------------------------------------------------------------------

/// Return the fill colour for a given node icon type.
pub fn icon_color(icon: NodeIcon) -> Color {
    match icon {
        NodeIcon::Folder => Color::rgb(0.90, 0.72, 0.20),
        NodeIcon::File => Color::rgb(0.55, 0.78, 0.95),
        NodeIcon::Package => Color::rgb(0.70, 0.60, 0.88),
    }
}

// ---------------------------------------------------------------------------
// ExpandToggle
// ---------------------------------------------------------------------------

/// Draws the ▶/▼ expand arrow. **Display-only** — returns `Ignored` for all events.
///
/// Interaction is handled centrally by `TreeView::on_event()`, which uses the
/// `RowMeta::toggle_rect` field (populated from `TreeRow::toggle_local_bounds` during
/// layout) to detect clicks on the toggle area and toggle `TreeNode::is_expanded` directly.
pub struct ExpandToggle {
    bounds: Rect,
    pub has_children: bool,
    pub is_expanded: bool,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
}

impl ExpandToggle {
    pub fn new(has_children: bool, is_expanded: bool) -> Self {
        Self {
            bounds: Rect::default(),
            has_children,
            is_expanded,
            children: Vec::new(),
            base: WidgetBase::new(),
        }
    }
}

impl Widget for ExpandToggle {
    fn type_name(&self) -> &'static str {
        "ExpandToggle"
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
        Size::new(EXPAND_W, available.height)
    }

    // The framework has already translated `ctx` to this widget's bottom-left origin.
    // All drawing coordinates are widget-local (0,0 = bottom-left of this widget).
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.has_children {
            return;
        }

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

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
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
    base: WidgetBase,
}

impl NodeIconWidget {
    pub fn new(icon: NodeIcon) -> Self {
        Self {
            bounds: Rect::default(),
            icon,
            children: Vec::new(),
            base: WidgetBase::new(),
        }
    }
}

impl Widget for NodeIconWidget {
    fn type_name(&self) -> &'static str {
        "NodeIconWidget"
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
        Size::new(ICON_W + ICON_GAP, available.height)
    }

    // The framework has already translated `ctx` to this widget's bottom-left origin.
    // All drawing coordinates are widget-local (0,0 = bottom-left of this widget).
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

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// TreeRow
// ---------------------------------------------------------------------------

/// Compositional row: `SizedBox` (indent) | `ExpandToggle` | `NodeIconWidget` | `Label`.
///
/// **Event-routing note:** `TreeRow` and its children all return `EventResult::Ignored`.
/// The containing `TreeView` handles all events (selection, expand/collapse) using its
/// `row_metas: Vec<RowMeta>` which records each row's node_idx and toggle bounds.
pub struct TreeRow {
    bounds: Rect,
    pub node_idx: usize,
    /// Bounds of the `ExpandToggle` in row-local coordinates (set in `layout()`).
    /// For leaf nodes (`has_children = false`), this field is `Rect::default()` (all zeros)
    /// and is never read — `TreeView` uses `None` for the corresponding `RowMeta::toggle_rect`.
    pub toggle_local_bounds: Rect,
    is_selected: bool,
    is_hovered: bool,
    focused: bool,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
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
            base: WidgetBase::new(),
        }
    }
}

impl Widget for TreeRow {
    fn type_name(&self) -> &'static str {
        "TreeRow"
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
        let v = ctx.visuals();

        if self.is_selected {
            let c = if self.focused {
                // Accent-tinted overlay — same colour in both themes so the
                // selection reads as "selected" regardless of palette.
                Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.25)
            } else {
                // Theme-neutral dim overlay: subtle tint of the text color.
                Color::rgba(v.text_color.r, v.text_color.g, v.text_color.b, 0.12)
            };
            ctx.set_fill_color(c);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, h);
            ctx.fill();
        } else if self.is_hovered {
            ctx.set_fill_color(Color::rgba(
                v.text_color.r,
                v.text_color.g,
                v.text_color.b,
                0.08,
            ));
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, h);
            ctx.fill();
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
