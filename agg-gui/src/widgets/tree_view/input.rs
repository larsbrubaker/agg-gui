//! Mouse and keyboard handling for `TreeView`: hover, click selection
//! (single / ctrl / shift), expand toggles, double-click activation,
//! scrollbar and node dragging, and arrow / Space / Enter navigation.
//!
//! Rows are located arithmetically from the scroll offset and row height
//! (the row widgets only exist for the viewport), using the cached visible
//! rows in `flat.rs`.  User-visible changes are reported through
//! `TreeView::emit` (see `api.rs`).  Split out of `mod.rs` (800-line cap).

use crate::event::{EventResult, Key, Modifiers};
use crate::geometry::Point;

use super::drag::{apply_drop, compute_drop_target};
use super::node::DragState;
use super::row::EXPAND_W;
use super::{TreeView, TreeViewEvent, DRAG_THRESHOLD};

impl TreeView {
    /// Flip `node_idx`'s expansion on the user's behalf and report it.
    fn toggle_expanded_by_user(&mut self, node_idx: usize) {
        let expanded = !self.nodes[node_idx].is_expanded;
        self.nodes[node_idx].is_expanded = expanded;
        self.emit(if expanded {
            TreeViewEvent::Expanded(node_idx)
        } else {
            TreeViewEvent::Collapsed(node_idx)
        });
    }

    fn emit_selection_changed(&mut self) {
        let cursor = self.cursor_node;
        self.emit(TreeViewEvent::SelectionChanged { cursor });
    }

    pub(super) fn handle_mouse_move(&mut self, pos: Point) -> EventResult {
        let old_hovered_scrollbar = self.hovered_scrollbar;
        let old_hovered_row = self.hovered_row;
        self.hovered_scrollbar = self.in_scrollbar(pos);

        if self.dragging_scrollbar {
            if let Some((_, thumb_h)) = self.thumb_metrics() {
                let h = self.bounds.height;
                let track_h = (h - thumb_h).max(1.0);
                let delta_y = self.sb_drag_start_y - pos.y;
                let spp = self.max_scroll() / track_h;
                self.scroll_offset =
                    (self.sb_drag_start_offset + delta_y * spp).clamp(0.0, self.max_scroll());
            }
            return EventResult::Consumed;
        }

        if let Some(drag) = &mut self.drag {
            let dx = pos.x - drag.current_pos.x;
            let dy = pos.y - drag.current_pos.y;
            drag.current_pos = pos;
            if !drag.live && (dx * dx + dy * dy).sqrt() > DRAG_THRESHOLD {
                drag.live = true;
            }
            if drag.live {
                self.flat.refresh(&self.nodes);
                if let Some(drag) = self.drag.as_ref() {
                    self.drop_target = compute_drop_target(
                        pos,
                        &self.flat.rows,
                        &self.nodes,
                        self.viewport_h,
                        self.row_height,
                        self.scroll_offset,
                        drag,
                    );
                }
            }
            return EventResult::Consumed;
        }

        self.hovered_row = self.row_index_at(pos);
        if self.hover_repaint
            && (self.hovered_scrollbar != old_hovered_scrollbar
                || self.hovered_row != old_hovered_row)
        {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    pub(super) fn handle_mouse_down(&mut self, pos: Point, mods: Modifiers) -> EventResult {
        if self.in_scrollbar(pos) {
            self.dragging_scrollbar = true;
            self.sb_drag_start_y = pos.y;
            self.sb_drag_start_offset = self.scroll_offset;
            return EventResult::Consumed;
        }

        self.refresh_flat();
        let Some(row_i) = self.row_index_at(pos) else {
            return EventResult::Ignored;
        };
        let Some(row) = self.display_row(row_i) else {
            return EventResult::Ignored;
        };
        let node_idx = row.node_idx;

        // Expand/collapse: any click on a row with children toggles it when
        // `toggle_on_row_click` is enabled (file-explorer style).  Otherwise
        // only the expand-toggle arrow triggers expansion so that clicking a
        // row in the inspector tree selects it without accidentally collapsing
        // a branch the user was browsing.
        let toggle_x = row.depth as f64 * self.indent_width;
        let on_toggle = pos.x >= toggle_x && pos.x < toggle_x + EXPAND_W;
        if row.has_children && (self.toggle_on_row_click || on_toggle) {
            self.toggle_expanded_by_user(node_idx);
        }

        // Selection
        let changed = if mods.ctrl {
            self.toggle_select(node_idx);
            true
        } else if mods.shift {
            match self.cursor_node {
                Some(a) => self.range_select(a, node_idx),
                None => self.set_single_selection(node_idx),
            }
        } else {
            let changed = self.set_single_selection(node_idx);
            if self.drag_enabled {
                let y_bot = self.row_y(row_i);
                self.drag = Some(DragState {
                    node_idx,
                    _cursor_row_offset: pos.y - y_bot,
                    current_pos: pos,
                    live: false,
                });
            }
            changed
        };
        if changed {
            self.emit_selection_changed();
        }
        if crate::event::current_click_count() >= 2 {
            self.emit(TreeViewEvent::Activated(node_idx));
        }

        EventResult::Consumed
    }

    pub(super) fn handle_mouse_up(&mut self, _pos: Point) -> EventResult {
        // Scrollbar drag end
        if self.dragging_scrollbar {
            self.dragging_scrollbar = false;
            return EventResult::Consumed;
        }

        // Node drag end
        if let Some(drag) = self.drag.take() {
            if drag.live {
                if let Some(target) = self.drop_target.take() {
                    apply_drop(&mut self.nodes, drag.node_idx, target);
                    self.counted_len = usize::MAX;
                }
            } else if self.set_single_selection(drag.node_idx) {
                // Was a click, not a drag — finalize single-select.
                self.emit_selection_changed();
            }
            self.drop_target = None;
            return EventResult::Consumed;
        }

        EventResult::Ignored
    }

    pub(super) fn handle_key_down(&mut self, key: &Key, _mods: Modifiers) -> EventResult {
        self.refresh_flat();
        let cursor_row = self
            .cursor_node
            .and_then(|ni| self.flat.position_of(ni))
            .map(|i| self.flat.rows[i]);
        match key {
            Key::ArrowDown | Key::ArrowUp => {
                let delta = if matches!(key, Key::ArrowDown) { 1 } else { -1 };
                if self.move_cursor(delta) {
                    self.emit_selection_changed();
                }
                EventResult::Consumed
            }
            Key::ArrowRight => {
                if let Some(row) = cursor_row {
                    if row.has_children && !self.nodes[row.node_idx].is_expanded {
                        self.toggle_expanded_by_user(row.node_idx);
                    } else if self.move_cursor(1) {
                        // Move to the first child (or the next row).
                        self.emit_selection_changed();
                    }
                }
                EventResult::Consumed
            }
            Key::ArrowLeft => {
                if let Some(ni) = self.cursor_node {
                    if self.nodes[ni].is_expanded {
                        self.toggle_expanded_by_user(ni);
                    } else if let Some(parent_idx) = self.nodes[ni].parent {
                        if parent_idx < self.nodes.len() {
                            if self.set_single_selection(parent_idx) {
                                self.emit_selection_changed();
                            }
                            if let Some(fi) = self.flat.position_of(parent_idx) {
                                self.reveal_row_now(fi);
                            }
                        }
                    }
                }
                EventResult::Consumed
            }
            Key::Char(' ') | Key::Enter => {
                let enter = matches!(key, Key::Enter);
                if let Some(row) = cursor_row {
                    if row.has_children && (!enter || self.enter_toggles_expansion) {
                        self.toggle_expanded_by_user(row.node_idx);
                    }
                    if enter {
                        self.emit(TreeViewEvent::Activated(row.node_idx));
                    }
                }
                EventResult::Consumed
            }
            Key::Tab => EventResult::Ignored, // let App handle focus advancement
            _ => EventResult::Ignored,
        }
    }
}
