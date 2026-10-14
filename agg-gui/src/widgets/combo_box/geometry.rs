//! Geometry of [`super::ComboBox`]'s open list: rows, scrolling, the
//! scroll bar and how much room the list takes. Popup positions are in the
//! popup frame ("F space", `popup_frame.rs`): `x` runs `0..width` across the
//! list and `y` is `0` at the drawn box edge the list hangs from, so the list
//! spans `popup_bottom()..popup_top()`. The closed box (`in_button`) stays in
//! the combo's local space.
//!
//! Scrolling is by whole rows (`scroll_offset`), except that opening centres
//! the selected row, which can leave the list a part row further on
//! (`scroll_frac`, C# `ScrollIntoView(…, ScrollAmount.Center)`); any other
//! scroll goes back to the row grid.

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
            self.popup_h()
        } else {
            0.0
        }
    }

    pub(super) fn popup_bottom(&self) -> f64 {
        self.popup_top() - self.popup_h()
    }

    /// Row `i`'s rectangle in F space, laid out from the top of the list
    /// whether or not it is scrolled into view.
    pub(super) fn row_rect(&self, i: usize) -> Rect {
        let above = i as f64 - self.scroll_offset as f64;
        Rect::new(
            0.0,
            self.popup_top() - (above + 1.0) * ITEM_H + self.scroll_frac,
            self.popup_width(),
            ITEM_H,
        )
    }

    /// Row `i`'s rectangle in the combo's local space, or an empty one for a
    /// row scrolled off above.
    pub(super) fn item_rect(&self, i: usize) -> Rect {
        if i < self.scroll_offset {
            return Rect::new(0.0, 0.0, 0.0, 0.0);
        }
        self.popup_rect_to_local(self.row_rect(i))
    }

    /// Rows painted (in part, past a centred opening's fraction) from
    /// `scroll_offset` on.
    pub(super) fn rows_in_view(&self) -> usize {
        self.popup_visible_count + usize::from(self.scroll_frac > 0.0)
    }

    /// Which dropdown item (if any) contains F-space point `p`.
    pub(super) fn item_for_pos(&self, p: Point) -> Option<usize> {
        if !self.open {
            return None;
        }
        if self.pos_in_scrollbar(p) {
            return None;
        }
        if !self.pos_in_popup(p) {
            return None;
        }
        let row = ((self.popup_top() - p.y + self.scroll_frac) / ITEM_H)
            .floor()
            .max(0.0) as usize;
        let idx = self.scroll_offset + row;
        (row < self.rows_in_view() && idx < self.options.len()).then_some(idx)
    }

    pub(super) fn in_button(&self, p: Point) -> bool {
        p.x >= 0.0 && p.x <= self.bounds.width && p.y >= 0.0 && p.y <= self.closed_h()
    }

    pub(super) fn ensure_selected_visible(&mut self) {
        self.ensure_index_visible(self.selected);
    }

    /// Scroll the open list the least that brings row `index` into view
    /// (C# `ScrollIntoView`'s default `ScrollAmount.Minimum`): keyboard
    /// stepping and type-ahead.
    pub(super) fn ensure_index_visible(&mut self, index: usize) {
        self.scroll_frac = 0.0;
        let n = self.options.len();
        if n == 0 {
            self.scroll_offset = 0;
            return;
        }
        let visible = self.popup_visible_count.max(1).min(n);
        if index < self.scroll_offset {
            self.scroll_offset = index;
        } else if index >= self.scroll_offset + visible {
            self.scroll_offset = index + 1 - visible;
        }
        self.scroll_offset = self.scroll_offset.min(n.saturating_sub(visible));
        self.sync_scrollbar_from_rows();
    }

    /// Scroll row `index` to the middle of the open list, as far as the
    /// list's ends allow (C# `ScrollIntoView(…, ScrollAmount.Center)`, which
    /// `DropDownList.ShowMenu` uses once as the list opens).
    pub(super) fn centre_index(&mut self, index: usize) {
        let n = self.options.len();
        let visible = self.popup_visible_count.max(1).min(n);
        let max_px = (n - visible.min(n)) as f64 * ITEM_H;
        let px = ((index as f64 + 0.5) * ITEM_H - visible as f64 * ITEM_H * 0.5).clamp(0.0, max_px);
        self.scroll_offset = (px / ITEM_H).floor() as usize;
        self.scroll_frac = px - self.scroll_offset as f64 * ITEM_H;
        self.sync_scrollbar_from_rows();
    }

    /// Where the list scrolls as it opens: the selected row centred, or the
    /// top when nothing is selected (C# opens a fresh, unscrolled list).
    pub(super) fn reveal_on_open(&mut self) {
        if self.has_selection {
            self.centre_index(self.selected);
        } else {
            self.scroll_offset = 0;
            self.scroll_frac = 0.0;
            self.sync_scrollbar_from_rows();
        }
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
        self.scrollbar.offset = self.scroll_offset as f64 * ITEM_H + self.scroll_frac;
        self.scrollbar.clamp_offset(self.popup_scroll_viewport());
    }

    pub(super) fn sync_rows_from_scrollbar(&mut self) {
        let n = self.options.len();
        let visible = self.popup_visible_count.max(1).min(n);
        let max_scroll = n.saturating_sub(visible);
        self.scroll_offset = ((self.scrollbar.offset / ITEM_H).round() as usize).min(max_scroll);
        self.scroll_frac = 0.0;
        self.scrollbar.offset = self.scroll_offset as f64 * ITEM_H;
    }

    pub(super) fn scrollbar_geometry(&self, style: ScrollBarStyle) -> ScrollbarGeometry {
        ScrollbarGeometry {
            orientation: ScrollbarOrientation::Vertical,
            track_start: self.popup_bottom() + style.inner_margin,
            track_end: self.popup_top() - style.inner_margin,
            cross_end: self.popup_width() - style.outer_margin,
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
            && p.x <= self.popup_width()
            && p.y >= self.popup_bottom()
            && p.y <= self.popup_top()
    }

    /// Size the open list and pick its side from the room on screen around
    /// the drawn box, whose `bottom` and `top` are measured from the bottom
    /// of a `viewport_h` tall visible area. As C#'s `RecalculatePosition`:
    /// the preferred side ([`ComboBox::with_opens_upward`]) when the list
    /// fits there, else the other side when it fits there, else whichever
    /// side has more room (the preferred one on a tie), scrolling.
    pub(super) fn configure_popup_geometry(&mut self, bottom: f64, top: f64, viewport_h: f64) {
        let n = self.options.len();
        if n == 0 {
            self.popup_visible_count = 0;
            self.popup_opens_up = false;
            self.scroll_offset = 0;
            self.scroll_frac = 0.0;
            return;
        }

        let desired_h = n as f64 * ITEM_H;
        let below = (bottom - POPUP_MARGIN).max(ITEM_H);
        let above = (viewport_h - top - POPUP_MARGIN).max(ITEM_H);
        let (preferred, opposite) = if self.prefer_up {
            (above, below)
        } else {
            (below, above)
        };
        let flip = desired_h > preferred && (desired_h <= opposite || opposite > preferred);
        self.popup_opens_up = self.prefer_up != flip;
        let mut available_h = if self.popup_opens_up { above } else { below };
        if self.max_height > 0.0 {
            available_h = available_h.min(self.max_height);
        }
        let fit_count = (available_h / ITEM_H).floor().max(1.0) as usize;
        let visible = if self.max_height > 0.0 {
            fit_count.min(n)
        } else {
            fit_count.clamp(MIN_VISIBLE_ITEMS.min(n), n)
        };
        // This runs every frame the list is open. Opening centres the
        // selection once its room is known; after that the selection is
        // re-revealed only when the room changed, otherwise a wheel or
        // scroll-bar scroll away from the selected row would snap back on
        // the next paint.
        let changed = visible != self.popup_visible_count;
        self.popup_visible_count = visible;
        if std::mem::take(&mut self.centre_on_open) {
            self.reveal_on_open();
        } else if changed {
            self.ensure_selected_visible();
        } else {
            let max_px = n.saturating_sub(visible) as f64 * ITEM_H;
            let px = (self.scroll_offset as f64 * ITEM_H + self.scroll_frac).min(max_px);
            self.scroll_offset = (px / ITEM_H).floor() as usize;
            self.scroll_frac = px - self.scroll_offset as f64 * ITEM_H;
        }
        self.sync_scrollbar_from_rows();
    }
}
