//! `Widget` impl for `InspectorPanel` — extracted from `mod.rs` to keep
//! the parent file under the project's 800-line cap.  All InspectorPanel
//! state and helpers still live in the parent module; this file only
//! routes the trait methods (layout / paint / event dispatch) into them.

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::{InspectorOverlay, Widget};
use crate::widgets::tree_view::{NodeIcon, TreeNode};

use super::{
    c_border, c_dim_text, c_header_bg, c_panel_bg, c_props_bg, c_split_bg, c_text, InspectorPanel,
    HEADER_H, MIN_PROPS_H, MIN_TREE_H,
};

impl Widget for InspectorPanel {
    fn type_name(&self) -> &'static str {
        "InspectorPanel"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self._children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self._children
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
        self.bounds.width = available.width;
        self.bounds.height = available.height;

        let nodes = self.nodes.borrow();
        // Fingerprint of the inspector_nodes Vec.  When the harness skips
        // a snapshot pass (e.g. during a window-resize drag) the Vec is
        // reused, so the data ptr stays the same — we then skip the
        // tree_view.nodes rebuild here.  Combined with TreeView's row
        // caching, this is what makes inspector window resizing cheap.
        let nodes_fingerprint = (nodes.as_ptr() as usize, nodes.len());
        let pending_state = self.pending_expanded.is_some() || self.pending_selected.is_some();
        let nodes_unchanged = !pending_state
            && self.last_inspector_nodes_fingerprint == Some(nodes_fingerprint)
            && !self.tree_view.nodes.is_empty();

        if !nodes_unchanged {
            // Preserve expansion/selection state by index before rebuilding.
            let mut old_expanded: Vec<bool> =
                self.tree_view.nodes.iter().map(|n| n.is_expanded).collect();
            let mut old_selected: Vec<bool> =
                self.tree_view.nodes.iter().map(|n| n.is_selected).collect();
            if let Some(pe) = self.pending_expanded.take() {
                old_expanded = pe;
            }
            if let Some(ps) = self.pending_selected.take() {
                old_selected = vec![false; old_expanded.len().max(ps.map(|i| i + 1).unwrap_or(0))];
                if let Some(i) = ps {
                    if i < old_selected.len() {
                        old_selected[i] = true;
                    }
                }
            }

            self.tree_view.nodes.clear();

            // Convert flat InspectorNode list (with depths) to parent-child
            // TreeNode structure via a depth stack.
            let mut depth_stack: Vec<usize> = Vec::new();
            let mut per_parent_counts: std::collections::HashMap<Option<usize>, u32> =
                std::collections::HashMap::new();

            for (orig_idx, node) in nodes.iter().enumerate() {
                let parent = if node.depth == 0 {
                    None
                } else {
                    depth_stack.get(node.depth.saturating_sub(1)).copied()
                };
                let order = {
                    let cnt = per_parent_counts.entry(parent).or_insert(0);
                    let o = *cnt;
                    *cnt += 1;
                    o
                };
                let b = &node.screen_bounds;
                let label = format!("{}  {:.0}×{:.0}", node.type_name, b.width, b.height);
                let tv_idx = self.tree_view.nodes.len();
                self.tree_view
                    .nodes
                    .push(TreeNode::new(label, NodeIcon::Package, parent, order));
                self.tree_view.nodes[tv_idx].is_expanded =
                    old_expanded.get(orig_idx).copied().unwrap_or(true);
                self.tree_view.nodes[tv_idx].is_selected =
                    old_selected.get(orig_idx).copied().unwrap_or(false);
                if depth_stack.len() <= node.depth {
                    depth_stack.resize(node.depth + 1, 0);
                }
                depth_stack[node.depth] = tv_idx;
            }
            self.last_inspector_nodes_fingerprint = Some(nodes_fingerprint);
        }

        self.selected = self.tree_view.nodes.iter().position(|n| n.is_selected);

        *self.hovered_bounds.borrow_mut() = self
            .tree_view
            .hovered_node_idx()
            .and_then(|i| nodes.get(i))
            .map(|n| InspectorOverlay {
                bounds: n.screen_bounds,
                margin: n.margin,
                padding: n.padding,
            });

        let tree_w = available.width;
        let tree_bot = self.tree_origin_y();
        let tree_top = self.list_area_h();
        let tree_h = (tree_top - tree_bot).max(0.0);
        self.tree_view
            .set_bounds(Rect::new(0.0, tree_bot, tree_w, tree_h));
        self.tree_view.layout(Size::new(tree_w, tree_h));

        // Keep the presence node's bounds in sync with the real TreeView so
        // the inspector displays accurate bounds for this proxy entry.
        self._children[0].set_bounds(self.tree_view.bounds());

        if let Some(cell) = &self.snapshot_out {
            *cell.borrow_mut() = Some(self.saved_state());
        }

        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let sy = self.split_y();
        let hdr_y = h - HEADER_H;
        let v = ctx.visuals().clone();

        // Panel background
        ctx.set_fill_color(c_panel_bg(&v));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        ctx.set_stroke_color(c_border(&v));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, 0.0);
        ctx.line_to(0.0, h);
        ctx.stroke();

        // Header
        ctx.set_fill_color(c_header_bg(&v));
        ctx.begin_path();
        ctx.rect(0.0, hdr_y, w, HEADER_H);
        ctx.fill();

        ctx.set_stroke_color(c_border(&v));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, hdr_y);
        ctx.line_to(w, hdr_y);
        ctx.stroke();

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(13.0);
        ctx.set_fill_color(c_text(&v));
        let title = "Widget Inspector";
        if let Some(m) = ctx.measure_text(title) {
            ctx.fill_text(
                title,
                12.0,
                hdr_y + (HEADER_H - m.ascent - m.descent) * 0.5 + m.descent,
            );
        }

        let count_txt = format!("{} widgets", self.nodes.borrow().len());
        ctx.set_font_size(11.0);
        ctx.set_fill_color(c_dim_text(&v));
        if let Some(m) = ctx.measure_text(&count_txt) {
            ctx.fill_text(
                &count_txt,
                w - m.width - 10.0,
                hdr_y + (HEADER_H - m.ascent - m.descent) * 0.5 + m.descent,
            );
        }

        // Properties pane
        ctx.set_fill_color(c_props_bg(&v));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, sy - 2.0);
        ctx.fill();
        self.paint_properties(ctx, sy - 2.0);

        // Split handle
        ctx.set_fill_color(c_split_bg(&v));
        ctx.begin_path();
        ctx.rect(0.0, sy - 2.0, w, 4.0);
        ctx.fill();
        ctx.set_stroke_color(c_border(&v));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, sy);
        ctx.line_to(w, sy);
        ctx.stroke();

        // Tree area
        let tree_bot = self.tree_origin_y();
        let tree_top = self.list_area_h();
        let tree_h = (tree_top - tree_bot).max(0.0);
        if tree_h > 0.0 {
            ctx.save();
            ctx.translate(0.0, tree_bot);
            ctx.clip_rect(0.0, 0.0, w, tree_h);
            crate::widget::paint_subtree(&mut self.tree_view, ctx);
            ctx.restore();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                if pos.y < self.split_y() - 2.0 && self.try_emit_base_edit_from_click(*pos) {
                    return EventResult::Consumed;
                }
                #[cfg(feature = "reflect")]
                if pos.y < self.split_y() - 2.0 && self.try_emit_edit_from_click(*pos) {
                    return EventResult::Consumed;
                }
                if self.on_split_handle(*pos) {
                    self.split_dragging = true;
                    return EventResult::Consumed;
                }
                if self.pos_in_tree_area(*pos) {
                    return self.forward_to_tree(event);
                }
                EventResult::Ignored
            }
            Event::MouseMove { pos } => {
                if self.split_dragging {
                    self.props_h = pos.y.clamp(
                        MIN_PROPS_H,
                        (self.list_area_h() - MIN_TREE_H).max(MIN_PROPS_H),
                    );
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                if self.pos_in_tree_area(*pos) {
                    let _ = self.forward_to_tree(event);
                    self.update_hovered_bounds_from_tree();
                } else if self.hovered_bounds.borrow().is_some() {
                    *self.hovered_bounds.borrow_mut() = None;
                    crate::animation::request_draw_without_invalidation();
                }
                EventResult::Ignored
            }
            Event::MouseUp {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                if self.split_dragging {
                    self.split_dragging = false;
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                if self.pos_in_tree_area(*pos) {
                    return self.forward_to_tree(event);
                }
                EventResult::Ignored
            }
            Event::MouseWheel { pos, .. } if self.pos_in_tree_area(*pos) => {
                self.forward_to_tree(event)
            }
            _ => EventResult::Ignored,
        }
    }
}
