//! `Widget` impl for `TreeView` — extracted from `mod.rs` to keep the
//! main file under the project's 800-line cap.  Layout is virtualised:
//! `layout()` builds `TreeRow` widgets only for the rows inside the viewport
//! and reuses a row's widget while its signature ([`row_signature`]) is
//! unchanged.  `paint()` draws the background, scrollbar, selection, hover
//! and drag feedback; the framework then paints the row widgets on top.
//! Input handling lives in `input.rs`, the row cache in `flat.rs`.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::icon_image::IconImage;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

use super::drag::{paint_drop_child_highlight, paint_drop_line, paint_ghost};
use super::node::{DropPosition, FlatRow, TreeNode};
use super::row::{icon_color, TreeRow, EXPAND_W};
use super::{RowMeta, TreeView, SCROLLBAR_W};

/// Hash of everything a row widget shows — its node's content and depth,
/// plus the tree-wide metrics and fonts.  Selection, hover and focus are
/// painted by `TreeView::paint`, so they are deliberately not part of it.
fn row_signature(tree: &TreeView, node: &TreeNode, flat: &FlatRow) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    node.label.hash(&mut h);
    flat.depth.hash(&mut h);
    flat.has_children.hash(&mut h);
    node.is_expanded.hash(&mut h);
    (node.icon as u8).hash(&mut h);
    node.icon_image
        .as_ref()
        .map(IconImage::identity)
        .hash(&mut h);
    if let Some(g) = node.icon_glyph {
        g.glyph.hash(&mut h);
        [g.color.r, g.color.g, g.color.b, g.color.a]
            .map(f32::to_bits)
            .hash(&mut h);
    }
    node.secondary_text.hash(&mut h);
    node.fraction.map(f32::to_bits).hash(&mut h);
    Arc::as_ptr(&tree.font).hash(&mut h);
    tree.icon_font.as_ref().map(Arc::as_ptr).hash(&mut h);
    tree.font_size.to_bits().hash(&mut h);
    tree.row_height.to_bits().hash(&mut h);
    tree.indent_width.to_bits().hash(&mut h);
    tree.name_ellipsis.hash(&mut h);
    h.finish()
}

impl TreeView {
    /// Build the `TreeRow` widget for `flat`.
    fn build_row(&self, flat: &FlatRow) -> TreeRow {
        let node = &self.nodes[flat.node_idx];
        let glyph_font = self.icon_font.as_ref().unwrap_or(&self.font);
        TreeRow::new(
            flat.node_idx,
            flat.depth,
            flat.has_children,
            node.is_expanded,
            false, // selection is painted by `TreeView::paint`
            false,
            node.icon,
            node.label.clone(),
            Arc::clone(&self.font),
            self.font_size,
            self.indent_width,
            self.row_height,
        )
        .with_icon_image(node.icon_image.clone())
        .with_icon_glyph(node.icon_glyph.map(|g| (g, Arc::clone(glyph_font))))
        .with_trailing(node.secondary_text.clone(), node.fraction)
        .with_name_ellipsis(self.name_ellipsis)
    }

    /// The hovered row's widget and node, unless a drag is under way.
    fn hovered_row_widget(&self) -> Option<(usize, &dyn Widget)> {
        if self.drag.is_some() {
            return None;
        }
        let node = self.hovered_node_idx()?;
        let k = self.row_metas.iter().position(|m| m.node_idx == node)?;
        Some((node, self.row_widgets.get(k)?.as_ref()))
    }

    /// The hovered row's tip — its full name while elided.
    fn hovered_row_tip(&self) -> Option<(usize, &str)> {
        let (node, row) = self.hovered_row_widget()?;
        Some((node, row.tooltip_text()?))
    }
}

impl Widget for TreeView {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "TreeView"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.row_widgets
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.row_widgets
    }
    fn is_focusable(&self) -> bool {
        true
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

    fn hit_test(&self, local_pos: Point) -> bool {
        // Capture all events during drags even if cursor leaves bounds.
        if self.drag.is_some() || self.dragging_scrollbar {
            return true;
        }
        let b = self.bounds();
        local_pos.x >= 0.0
            && local_pos.x <= b.width
            && local_pos.y >= 0.0
            && local_pos.y <= b.height
    }

    /// The hovered row's elided name (see [`TreeView::name_ellipsis`]),
    /// else the tree's own tip.
    fn tooltip_text(&self) -> Option<&str> {
        match self.hovered_row_tip() {
            Some((_, tip)) => Some(tip),
            None => self.base.tooltip.as_deref(),
        }
    }

    /// The hovered node while its row tips, so moving to another elided row
    /// re-arms the tooltip.
    fn tooltip_key(&self) -> Option<u64> {
        self.hovered_row_tip().map(|(node, _)| node as u64)
    }

    fn claims_pointer_exclusively(&self, _local_pos: Point) -> bool {
        // Rows are display-only: every press must reach (and focus) the
        // tree itself, not a row's child label.
        true
    }

    fn layout(&mut self, available: Size) -> Size {
        self.refresh_flat();
        let h = available.height;
        let w = available.width - SCROLLBAR_W;
        let rh = self.row_height;
        self.viewport_h = h;
        self.content_height = self.flat.rows.len() as f64 * rh;
        self.apply_pending_scroll();
        self.scroll_offset = self
            .scroll_offset
            .clamp(0.0, (self.content_height - h).max(0.0));

        // Only the rows inside the viewport get a widget.
        let display_len = self.display_len();
        let (first, end) = if rh > 0.0 && h > 0.0 {
            let first = (self.scroll_offset / rh).floor().max(0.0) as usize;
            let end = ((self.scroll_offset + h) / rh).ceil().max(0.0) as usize;
            (first.min(display_len), end.min(display_len))
        } else {
            (0, 0)
        };

        // Keep each row widget whose content is unchanged (its labels keep
        // their rasters); build the rest.
        let mut old: Vec<(RowMeta, Box<dyn Widget>)> = std::mem::take(&mut self.row_metas)
            .into_iter()
            .zip(std::mem::take(&mut self.row_widgets))
            .collect();
        for i in first..end {
            let Some(flat) = self.display_row(i) else {
                break;
            };
            let sig = row_signature(self, &self.nodes[flat.node_idx], &flat);
            let reused = old
                .iter()
                .position(|(m, _)| m.node_idx == flat.node_idx && m.sig == sig)
                .map(|k| old.swap_remove(k).1);
            let mut row = reused.unwrap_or_else(|| Box::new(self.build_row(&flat)));
            let y_bot = self.row_y(i);
            row.layout(Size::new(w, rh));
            row.set_bounds(Rect::new(0.0, y_bot, w, rh));
            self.row_metas.push(RowMeta {
                node_idx: flat.node_idx,
                sig,
            });
            self.row_widgets.push(row);
        }

        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let h = self.bounds.height;
        let w = self.bounds.width;
        let content_w = w - SCROLLBAR_W;
        let v = ctx.visuals().clone();

        // Background — follow the theme's window fill rather than hard-coded white.
        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // Scrollbar — theme-aware track and thumb.
        let sb_x = self.scrollbar_x();
        if self.content_height > h {
            ctx.set_fill_color(v.scroll_track);
            ctx.begin_path();
            ctx.rect(sb_x, 0.0, SCROLLBAR_W, h);
            ctx.fill();
            if let Some((thumb_y, thumb_h)) = self.thumb_metrics() {
                let thumb_color = if self.dragging_scrollbar {
                    v.scroll_thumb_dragging
                } else if self.hovered_scrollbar {
                    v.scroll_thumb_hovered
                } else {
                    v.scroll_thumb
                };
                ctx.set_fill_color(thumb_color);
                ctx.begin_path();
                ctx.rounded_rect(sb_x + 2.0, thumb_y, SCROLLBAR_W - 4.0, thumb_h, 3.0);
                ctx.fill();
            }
        }

        // Content clip — rows must not bleed into the scrollbar strip.
        // This clip is active during framework recursion into row_widgets (after paint() returns).
        ctx.clip_rect(0.0, 0.0, content_w, h);

        // Selection and hover backgrounds — painted here, not by the
        // `TreeRow` widgets, so changing them never rebuilds a row (whose
        // labels keep their cached rasters).  Framework recursion paints
        // each row's content on top.  Hover is skipped on a selected row
        // (the selection tint already marks it).
        let selected_fill = if self.focused {
            // Accent-tinted overlay — same colour in both themes so the
            // selection reads as "selected" regardless of palette.
            Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.25)
        } else {
            // Theme-neutral dim overlay: subtle tint of the text color.
            Color::rgba(v.text_color.r, v.text_color.g, v.text_color.b, 0.12)
        };
        let hover_fill = Color::rgba(v.text_color.r, v.text_color.g, v.text_color.b, 0.08);
        let hovered_node = self.hovered_node_idx();
        for (meta, row) in self.row_metas.iter().zip(&self.row_widgets) {
            let is_sel = self.nodes.get(meta.node_idx).is_some_and(|n| n.is_selected);
            let fill = if is_sel {
                selected_fill
            } else if hovered_node == Some(meta.node_idx) {
                hover_fill
            } else {
                continue;
            };
            let rb = row.bounds();
            ctx.set_fill_color(fill);
            ctx.begin_path();
            ctx.rect(rb.x, rb.y, rb.width, rb.height);
            ctx.fill();
        }

        // Drop indicator and ghost (drag feedback)
        let rows = &self.flat.rows;
        if let Some(drop_target) = self.drop_target {
            if self.drag.as_ref().is_some_and(|d| d.live) {
                let rh = self.row_height;
                let off = self.scroll_offset;
                let ind = self.indent_width;
                let ref_node = match drop_target {
                    DropPosition::Before(ni)
                    | DropPosition::After(ni)
                    | DropPosition::AsChild(ni) => ni,
                };
                if let Some(ri) = rows.iter().position(|r| r.node_idx == ref_node) {
                    let y_bot = self.viewport_h - (ri as f64 + 1.0) * rh + off;
                    let indent = rows[ri].depth as f64 * ind + EXPAND_W;
                    match drop_target {
                        DropPosition::Before(_) => {
                            paint_drop_line(ctx, indent, y_bot + rh, content_w - indent)
                        }
                        DropPosition::After(_) => {
                            paint_drop_line(ctx, indent, y_bot, content_w - indent)
                        }
                        DropPosition::AsChild(_) => {
                            paint_drop_child_highlight(ctx, y_bot, content_w, rh)
                        }
                    }
                }
            }
        }
        if let Some(drag) = &self.drag {
            if drag.live {
                let label = self.nodes[drag.node_idx].label.clone();
                let ic = icon_color(self.nodes[drag.node_idx].icon);
                let pos = drag.current_pos;
                let rh = self.row_height;
                let font = Arc::clone(&self.font);
                let fs = self.font_size;
                paint_ghost(ctx, &label, pos, content_w, rh, &font, fs, ic);
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        // Every consumed event in a tree view mutates some visible state —
        // selection, expansion, scroll offset, hover row, focus ring.  Wrap
        // the dispatch so a `Consumed` result translates to a repaint
        // request.  Events that bubble away as `Ignored` do NOT tick,
        // honouring the "only repaint on real change" contract.
        let result = match event {
            Event::FocusGained => {
                self.focused = true;
                EventResult::Consumed
            }
            Event::FocusLost => {
                self.focused = false;
                EventResult::Consumed
            }

            Event::MouseWheel { delta_y, .. } => {
                // Convention (matches winit / WheelEvent after OS
                // natural-scroll): positive delta_y = user wants to
                // see content ABOVE = DECREASE scroll_offset.
                self.scroll_offset =
                    (self.scroll_offset - delta_y * 40.0).clamp(0.0, self.max_scroll());
                self.hovered_row = None;
                EventResult::Consumed
            }

            Event::MouseMove { pos } => self.handle_mouse_move(*pos),
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                modifiers,
            } => self.handle_mouse_down(*pos, *modifiers),
            Event::MouseUp {
                button: MouseButton::Left,
                pos,
                ..
            } => self.handle_mouse_up(*pos),
            Event::KeyDown { key, modifiers } => self.handle_key_down(key, *modifiers),
            _ => EventResult::Ignored,
        };
        if result.is_consumed() {
            crate::animation::request_draw();
        }
        result
    }
}
