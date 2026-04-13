//! Drag-and-drop helpers for `TreeView`: drop-target computation, node
//! reparenting, and paint helpers for the indicator and ghost.

use std::sync::Arc;

use crate::color::Color;
use crate::geometry::Point;
use crate::draw_ctx::DrawCtx;
use crate::text::Font;

use super::node::{
    DropPosition, DragState, FlatRow, TreeNode, is_descendant,
};

// ---------------------------------------------------------------------------
// Drop-target computation
// ---------------------------------------------------------------------------

const ZONE_EDGE: f64 = 0.28; // top/bottom fraction for before/after zones

/// Determine the drop position given the drag cursor and visible rows.
///
/// Returns `None` if the cursor is not over a valid drop target (e.g. over
/// the dragged node itself or one of its descendants).
pub fn compute_drop_target(
    pos: Point,
    rows: &[FlatRow],
    nodes: &[TreeNode],
    viewport_height: f64,
    row_height: f64,
    scroll_offset: f64,
    drag: &DragState,
) -> Option<DropPosition> {
    if rows.is_empty() {
        return None;
    }

    // Convert Y to flat-row index.
    let raw = (viewport_height - pos.y + scroll_offset) / row_height;
    if raw < 0.0 {
        return None;
    }
    let row_i = (raw as usize).min(rows.len() - 1);
    let target_node = rows[row_i].node_idx;

    // Can't drop onto self or own descendants.
    if target_node == drag.node_idx
        || is_descendant(nodes, drag.node_idx, target_node)
    {
        return None;
    }

    // Within-row zone: fraction from the bottom of this row.
    let row_y_bottom = viewport_height - (row_i as f64 + 1.0) * row_height + scroll_offset;
    let frac = (pos.y - row_y_bottom) / row_height; // 0=bottom edge, 1=top edge

    let pos = if frac < ZONE_EDGE {
        // Bottom zone → After
        DropPosition::After(target_node)
    } else if frac > 1.0 - ZONE_EDGE {
        // Top zone → Before
        DropPosition::Before(target_node)
    } else {
        // Middle zone → AsChild if it's a folder, else After
        if rows[row_i].has_children || matches!(
            nodes[target_node].icon,
            crate::widgets::tree_view::node::NodeIcon::Folder
                | crate::widgets::tree_view::node::NodeIcon::Package,
        ) {
            DropPosition::AsChild(target_node)
        } else {
            DropPosition::After(target_node)
        }
    };

    Some(pos)
}

// ---------------------------------------------------------------------------
// Apply drop (reparent + reorder)
// ---------------------------------------------------------------------------

/// Move `drag_node_idx` to the position described by `target`.
pub fn apply_drop(nodes: &mut Vec<TreeNode>, drag_node_idx: usize, target: DropPosition) {
    match target {
        DropPosition::AsChild(parent_idx) => {
            // Find the max order among existing children, append after them.
            let max_order = nodes
                .iter()
                .enumerate()
                .filter(|(i, n)| n.parent == Some(parent_idx) && *i != drag_node_idx)
                .map(|(_, n)| n.order)
                .max()
                .map(|o| o + 1)
                .unwrap_or(0);
            nodes[drag_node_idx].parent = Some(parent_idx);
            nodes[drag_node_idx].order = max_order;
            // Ensure the parent is expanded so the dropped node is visible.
            nodes[parent_idx].is_expanded = true;
        }
        DropPosition::Before(ref_node_idx) | DropPosition::After(ref_node_idx) => {
            let new_parent = nodes[ref_node_idx].parent;
            nodes[drag_node_idx].parent = new_parent;

            // Collect siblings (excluding the dragged node), sorted by order.
            let mut sibs: Vec<usize> = nodes
                .iter()
                .enumerate()
                .filter(|(i, n)| n.parent == new_parent && *i != drag_node_idx)
                .map(|(i, _)| i)
                .collect();
            sibs.sort_by_key(|&i| nodes[i].order);

            // Find insertion position.
            let ref_pos = sibs.iter().position(|&i| i == ref_node_idx).unwrap_or(0);
            let insert_at = match target {
                DropPosition::Before(_) => ref_pos,
                _ => ref_pos + 1,
            };
            sibs.insert(insert_at, drag_node_idx);

            // Renumber all siblings consecutively.
            for (new_order, &idx) in sibs.iter().enumerate() {
                nodes[idx].order = new_order as u32;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Paint helpers
// ---------------------------------------------------------------------------

/// Paint a horizontal drop-before/after indicator line.
pub fn paint_drop_line(ctx: &mut dyn DrawCtx, x: f64, y: f64, width: f64) {
    ctx.set_stroke_color(Color::rgb(0.22, 0.45, 0.88));
    ctx.set_line_width(2.0);
    ctx.begin_path();
    ctx.circle(x + 4.0, y, 3.0);
    ctx.fill();
    ctx.set_fill_color(Color::rgb(0.22, 0.45, 0.88));
    ctx.begin_path();
    ctx.move_to(x + 4.0, y);
    ctx.line_to(x + width, y);
    ctx.stroke();
}

/// Paint a full-row "drop as child" highlight.
pub fn paint_drop_child_highlight(ctx: &mut dyn DrawCtx, y_bottom: f64, width: f64, height: f64) {
    ctx.set_stroke_color(Color::rgba(0.22, 0.45, 0.88, 0.7));
    ctx.set_line_width(1.5);
    ctx.begin_path();
    ctx.rounded_rect(2.0, y_bottom, width - 4.0, height, 3.0);
    ctx.stroke();
}

/// Paint a semi-transparent ghost of the dragged row at the cursor position.
pub fn paint_ghost(
    ctx: &mut dyn DrawCtx,
    label: &str,
    pos: Point,
    width: f64,
    row_height: f64,
    font: &Arc<Font>,
    font_size: f64,
    icon_color: Color,
) {
    let gx = (pos.x - 12.0).max(0.0);
    let gy = pos.y - row_height * 0.5;

    // Shadow
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.18));
    ctx.begin_path();
    ctx.rounded_rect(gx + 2.0, gy - 2.0, width.min(200.0), row_height, 4.0);
    ctx.fill();

    // Ghost background
    ctx.set_global_alpha(0.82);
    ctx.set_fill_color(Color::rgb(0.97, 0.97, 1.0));
    ctx.begin_path();
    ctx.rounded_rect(gx, gy, width.min(200.0), row_height, 4.0);
    ctx.fill();

    // Icon
    ctx.set_fill_color(icon_color);
    ctx.begin_path();
    ctx.rounded_rect(gx + 6.0, gy + (row_height - 12.0) * 0.5, 12.0, 12.0, 2.0);
    ctx.fill();

    // Label
    ctx.set_font(Arc::clone(font));
    ctx.set_font_size(font_size);
    ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.87));
    if let Some(m) = ctx.measure_text(label) {
        let ty = gy + (row_height - m.ascent - m.descent) * 0.5 + m.descent;
        ctx.fill_text(label, gx + 24.0, ty);
    }

    ctx.set_global_alpha(1.0);
}



