//! `Widget` impl for `TreeView` — extracted from `mod.rs` to keep the
//! main file under the project's 800-line cap.  All TreeView logic
//! still lives in `mod.rs`; this submodule only routes the trait
//! methods (layout / paint / event dispatch / focus / hit-test) into
//! the helpers TreeView already exposes.

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

use super::drag::{paint_drop_child_highlight, paint_drop_line, paint_ghost};
use super::node::{flatten_visible, DropPosition, FlatRow};
use super::row::{icon_color, TreeRow, EXPAND_W};
use super::{RowMeta, TreeView, SCROLLBAR_W};

impl Widget for TreeView {
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

    fn layout(&mut self, available: Size) -> Size {
        let rows = flatten_visible(&self.nodes);
        self.content_height = rows.len() as f64 * self.row_height;
        self.scroll_offset = self.scroll_offset.clamp(0.0, self.max_scroll());

        let h = available.height;
        let w = available.width - SCROLLBAR_W;
        let rh = self.row_height;
        let ind = self.indent_width;
        let font_size = self.font_size;

        // Reuse cached rows when the row content is unchanged from the
        // previous layout — happens every frame of a window-resize drag.
        // We only reposition the existing TreeRow widgets and refresh the
        // toggle_rects, preserving each TreeRow's child Label backbuffers.
        // Without this, resizing a window with the inspector open
        // re-rasterised every label every frame.
        let visible_rows: Vec<&FlatRow> = rows
            .iter()
            .filter(|flat| {
                !self
                    .drag
                    .as_ref()
                    .map_or(false, |d| d.live && d.node_idx == flat.node_idx)
            })
            .collect();
        let new_sig = self.row_content_signature();
        let can_reuse = self.last_row_content_sig == Some(new_sig)
            && self.row_widgets.len() == visible_rows.len()
            && !self.row_widgets.is_empty();

        if can_reuse {
            // Reposition existing rows in place — no allocations, no
            // text re-rasterisation.
            for (i, flat) in visible_rows.iter().enumerate() {
                let y_bot = h - (i as f64 + 1.0) * rh + self.scroll_offset;
                let row = &mut self.row_widgets[i];
                row.layout(Size::new(w, rh));
                row.set_bounds(Rect::new(0.0, y_bot, w, rh));
                if let Some(meta) = self.row_metas.get_mut(i) {
                    debug_assert_eq!(meta.node_idx, flat.node_idx);
                    if let Some(ref mut tr) = meta.toggle_rect {
                        tr.y = y_bot + (rh - tr.height) * 0.5;
                    }
                }
            }
            return available;
        }

        // Full rebuild path — content has changed since the last layout.
        self.row_widgets.clear();
        self.row_metas.clear();

        for (i, flat) in visible_rows.iter().enumerate() {
            let node = &self.nodes[flat.node_idx];
            let y_bot = h - (i as f64 + 1.0) * rh + self.scroll_offset;
            let mut tree_row = TreeRow::new(
                flat.node_idx,
                flat.depth,
                flat.has_children,
                node.is_expanded,
                node.is_selected,
                self.focused,
                node.icon,
                node.label.clone(),
                Arc::clone(&self.font),
                font_size,
                ind,
                rh,
            );

            tree_row.layout(Size::new(w, rh));
            tree_row.set_bounds(Rect::new(0.0, y_bot, w, rh));

            let toggle_rect = if flat.has_children {
                let tlb = tree_row.toggle_local_bounds;
                Some(Rect::new(tlb.x, y_bot + tlb.y, tlb.width, tlb.height))
            } else {
                None
            };

            self.row_metas.push(RowMeta {
                node_idx: flat.node_idx,
                toggle_rect,
            });
            self.row_widgets.push(Box::new(tree_row));
        }
        self.last_row_content_sig = Some(new_sig);

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

        // Hover background — painted here (not on the individual `TreeRow`
        // widgets) so a hover flip doesn't have to invalidate the row's
        // cached label backbuffers.  Framework recursion paints each row's
        // content on top of this band.  Skip when the row is also
        // selected (the selection tint already conveys focus).
        if let Some(hi) = self.hovered_row {
            if let (Some(meta), Some(row_widget)) =
                (self.row_metas.get(hi), self.row_widgets.get(hi))
            {
                let is_sel = self
                    .nodes
                    .get(meta.node_idx)
                    .map(|n| n.is_selected)
                    .unwrap_or(false);
                if !is_sel {
                    let rb = row_widget.bounds();
                    ctx.set_fill_color(crate::color::Color::rgba(
                        v.text_color.r,
                        v.text_color.g,
                        v.text_color.b,
                        0.08,
                    ));
                    ctx.begin_path();
                    ctx.rect(rb.x, rb.y, rb.width, rb.height);
                    ctx.fill();
                }
            }
        }

        // Drop indicator and ghost (drag feedback)
        let rows = flatten_visible(&self.nodes);
        if let Some(drop_target) = self.drop_target {
            if self.drag.as_ref().map_or(false, |d| d.live) {
                let rh = self.row_height;
                let off = self.scroll_offset;
                let ind = self.indent_width;
                let ref_node = match drop_target {
                    DropPosition::Before(ni)
                    | DropPosition::After(ni)
                    | DropPosition::AsChild(ni) => ni,
                };
                if let Some(ri) = rows.iter().position(|r| r.node_idx == ref_node) {
                    let y_bot = h - (ri as f64 + 1.0) * rh + off;
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
        if result == EventResult::Consumed {
            crate::animation::request_draw();
        }
        result
    }
}
