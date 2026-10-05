use super::*;

impl ComboBox {
    // ── Internal helpers ─────────────────────────────────────────────────────

    pub(super) fn fire(&mut self) {
        let idx = self.selected;
        if let Some(cell) = &self.selected_cell {
            cell.set(idx);
        }
        if let Some(cb) = self.on_change.as_mut() {
            cb(idx);
        }
    }

    pub(super) fn popup_h(&self) -> f64 {
        self.popup_visible_count.min(self.options.len()) as f64 * ITEM_H
    }

    pub(super) fn popup_top(&self) -> f64 {
        if self.popup_opens_up {
            self.closed_h() + self.popup_h()
        } else {
            0.0
        }
    }

    pub(super) fn popup_bottom(&self) -> f64 {
        self.popup_top() - self.popup_h()
    }

    pub(super) fn item_rect(&self, i: usize) -> Rect {
        let Some(row) = i.checked_sub(self.scroll_offset) else {
            return Rect::new(0.0, 0.0, 0.0, 0.0);
        };
        let w = self.bounds.width;
        Rect::new(
            0.0,
            self.popup_top() - (row as f64 + 1.0) * ITEM_H,
            w,
            ITEM_H,
        )
    }

    /// Which dropdown item (if any) contains local point `p`.
    pub(super) fn item_for_pos(&self, p: Point) -> Option<usize> {
        if !self.open {
            return None;
        }
        if self.pos_in_scrollbar(p) {
            return None;
        }
        if p.x < 0.0
            || p.x > self.bounds.width
            || p.y < self.popup_bottom()
            || p.y > self.popup_top()
        {
            return None;
        }
        let row = ((self.popup_top() - p.y) / ITEM_H).floor().max(0.0) as usize;
        let idx = self.scroll_offset + row;
        (row < self.popup_visible_count && idx < self.options.len()).then_some(idx)
    }

    pub(super) fn in_button(&self, p: Point) -> bool {
        p.x >= 0.0 && p.x <= self.bounds.width && p.y >= 0.0 && p.y <= self.closed_h()
    }

    pub(super) fn ensure_selected_visible(&mut self) {
        let n = self.options.len();
        if n == 0 {
            self.scroll_offset = 0;
            return;
        }
        let visible = self.popup_visible_count.max(1).min(n);
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + visible {
            self.scroll_offset = self.selected + 1 - visible;
        }
        self.scroll_offset = self.scroll_offset.min(n.saturating_sub(visible));
        self.sync_scrollbar_from_rows();
    }

    pub(super) fn popup_scroll_viewport(&self) -> f64 {
        self.popup_h()
    }

    pub(super) fn popup_scroll_style(&self) -> ScrollBarStyle {
        current_scroll_style()
    }

    pub(super) fn sync_scrollbar_from_rows(&mut self) {
        self.scrollbar.enabled = true;
        self.scrollbar.content = self.options.len() as f64 * ITEM_H;
        self.scrollbar.offset = self.scroll_offset as f64 * ITEM_H;
        self.scrollbar.clamp_offset(self.popup_scroll_viewport());
    }

    pub(super) fn sync_rows_from_scrollbar(&mut self) {
        let n = self.options.len();
        let visible = self.popup_visible_count.max(1).min(n);
        let max_scroll = n.saturating_sub(visible);
        self.scroll_offset = ((self.scrollbar.offset / ITEM_H).round() as usize).min(max_scroll);
        self.scrollbar.offset = self.scroll_offset as f64 * ITEM_H;
    }

    pub(super) fn scrollbar_geometry(&self, style: ScrollBarStyle) -> ScrollbarGeometry {
        ScrollbarGeometry {
            orientation: ScrollbarOrientation::Vertical,
            track_start: self.popup_bottom() + style.inner_margin,
            track_end: self.popup_top() - style.inner_margin,
            cross_end: self.bounds.width - style.outer_margin,
            hit_margin: DEFAULT_GRAB_MARGIN,
        }
    }

    pub(super) fn pos_on_scroll_thumb(&self, p: Point) -> bool {
        let style = self.popup_scroll_style();
        self.scrollbar.pos_on_thumb(
            p,
            self.popup_scroll_viewport(),
            style,
            self.scrollbar_geometry(style),
        )
    }

    pub(super) fn pos_in_scrollbar(&self, p: Point) -> bool {
        let style = self.popup_scroll_style();
        self.scrollbar
            .pos_in_hover(p, style, self.scrollbar_geometry(style))
            && self.scrollbar.can_scroll(self.popup_scroll_viewport())
    }

    pub(super) fn pos_in_popup(&self, p: Point) -> bool {
        self.open
            && p.x >= 0.0
            && p.x <= self.bounds.width
            && p.y >= self.popup_bottom()
            && p.y <= self.popup_top()
    }

    pub(super) fn configure_popup_geometry(&mut self, origin_y: f64, viewport_h: f64) {
        let n = self.options.len();
        if n == 0 {
            self.popup_visible_count = 0;
            self.popup_opens_up = false;
            self.scroll_offset = 0;
            return;
        }

        let desired_h = n as f64 * ITEM_H;
        let below = (origin_y - POPUP_MARGIN).max(ITEM_H);
        let above = (viewport_h - (origin_y + self.closed_h()) - POPUP_MARGIN).max(ITEM_H);
        self.popup_opens_up = below < desired_h && above > below;
        let available_h = if self.popup_opens_up { above } else { below };
        let fit_count = (available_h / ITEM_H).floor().max(1.0) as usize;
        self.popup_visible_count = fit_count.clamp(MIN_VISIBLE_ITEMS.min(n), n);
        self.ensure_selected_visible();
        self.sync_scrollbar_from_rows();
    }
}
