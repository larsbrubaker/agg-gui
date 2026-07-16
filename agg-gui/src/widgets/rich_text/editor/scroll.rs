//! Internal vertical scrolling for [`RichTextEdit`], composing the shared
//! [`ScrollbarAxis`](crate::widgets::scrollbar::ScrollbarAxis) exactly as
//! `TextArea` does, so overflow math, thumb geometry, drag/hover and painting
//! stay pixel-consistent with every other scroll bar in the app.
//!
//! The scroll offset (`vbar.offset`) is folded into
//! [`doc_top_yup`](RichTextEdit::doc_top_yup), so all geometry (paint,
//! hit-test, caret) inherits scrolling for free.

use crate::draw_ctx::DrawCtx;
use crate::geometry::Point;
use crate::widgets::scroll_view::{
    current_scroll_style, current_scroll_visibility, ScrollBarStyle, ScrollBarVisibility,
};
use crate::widgets::scrollbar::{
    paint_prepared_scrollbar, ScrollbarGeometry, ScrollbarOrientation, DEFAULT_GRAB_MARGIN,
};

use super::super::model::DocPos;
use super::RichTextEdit;

/// Outcome of routing a `MouseMove` through the scroll bar first.
pub(crate) enum ScrollMove {
    /// The bar thumb is being dragged; `bool` = offset changed this move.
    Dragging(bool),
    /// Not dragging; `bool` = the bar's hover state changed this move.
    Hover(bool),
}

impl RichTextEdit {
    /// Furthest the content can scroll (0 when everything fits).
    pub(crate) fn max_scroll_y(&self) -> f64 {
        (self.content_height() - self.inner_height()).max(0.0)
    }

    /// Current vertical scroll offset (0 = top). Exposed for tests/inspectors.
    pub fn scroll_offset(&self) -> f64 {
        self.vbar.offset
    }

    fn scroll_style(&self) -> ScrollBarStyle {
        current_scroll_style()
    }
    fn scroll_visibility(&self) -> ScrollBarVisibility {
        current_scroll_visibility()
    }

    /// Refresh the axis with the current content height and re-clamp the offset.
    pub(crate) fn sync_scroll(&mut self) {
        self.vbar.enabled = true;
        self.vbar.content = self.content_height();
        self.vbar.clamp_offset(self.inner_height());
    }

    fn v_scrollbar_geometry(&self) -> ScrollbarGeometry {
        ScrollbarGeometry {
            orientation: ScrollbarOrientation::Vertical,
            track_start: self.padding,
            track_end: (self.bounds.height - self.padding).max(self.padding),
            cross_end: (self.bounds.width - 2.0).max(0.0),
            hit_margin: DEFAULT_GRAB_MARGIN,
        }
    }

    /// Scroll so the caret's visual line is fully visible.
    pub(crate) fn ensure_caret_visible(&mut self) {
        let caret = self.core.borrow().caret();
        let vlines = self.visual_lines();
        let vi = self.caret_visual_line(caret);
        let Some(&(_, _, y_top, height)) = vlines.get(vi) else {
            return;
        };
        let inner_h = self.inner_height();
        let top = y_top;
        let bottom = y_top + height;
        let mut off = self.vbar.offset;
        if top < off {
            off = top;
        } else if bottom > off + inner_h {
            off = bottom - inner_h;
        }
        self.vbar.offset = off.clamp(0.0, self.max_scroll_y());
    }

    pub(crate) fn scroll_by_wheel(&mut self, delta_y: f64) -> bool {
        if self.max_scroll_y() <= 0.0 {
            return false;
        }
        self.vbar.scroll_by(-delta_y, self.inner_height())
    }

    pub(crate) fn scrollbar_begin_drag(&mut self, pos: Point) -> bool {
        if self.max_scroll_y() <= 0.0 {
            return false;
        }
        let geom = self.v_scrollbar_geometry();
        self.vbar
            .begin_drag(pos, self.inner_height(), self.scroll_style(), geom)
    }

    pub(crate) fn scrollbar_on_mouse_move(&mut self, pos: Point) -> ScrollMove {
        let geom = self.v_scrollbar_geometry();
        let vp = self.inner_height();
        let style = self.scroll_style();
        if self.vbar.dragging {
            ScrollMove::Dragging(self.vbar.drag_to(pos, vp, style, geom))
        } else {
            ScrollMove::Hover(self.vbar.update_hover(pos, vp, style, geom))
        }
    }

    pub(crate) fn scrollbar_end_drag(&mut self) -> bool {
        let was = self.vbar.dragging;
        self.vbar.dragging = false;
        was
    }

    pub(crate) fn paint_scrollbar(&mut self, ctx: &mut dyn DrawCtx) {
        let vp = self.inner_height();
        let style = self.scroll_style();
        let vis = self.scroll_visibility();
        let geom = self.v_scrollbar_geometry();
        if let Some(bar) = self.vbar.prepare_paint(vp, style, vis, geom) {
            paint_prepared_scrollbar(ctx, bar);
        }
    }

    pub(crate) fn scrollbar_animating(&self) -> bool {
        self.vbar.animation_active()
    }

    /// Scroll the caret into view after an edit or navigation keystroke, given
    /// the (possibly just-moved) caret position.
    pub(crate) fn ensure_pos_visible(&mut self, _caret: DocPos) {
        self.ensure_caret_visible();
    }
}
