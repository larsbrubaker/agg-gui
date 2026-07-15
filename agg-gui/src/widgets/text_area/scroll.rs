//! Internal vertical scrolling for [`TextArea`], split out of `text_area.rs`
//! to keep that file under the 800-line cap.
//!
//! Rather than embedding a `ScrollView`, `TextArea` composes the same
//! [`ScrollbarAxis`](crate::widgets::scrollbar::ScrollbarAxis) primitive that
//! `ScrollView` uses. That keeps the overflow math, thumb geometry, drag/hover
//! interaction and painting pixel-consistent with every other scroll bar in the
//! app, and lets the widget read the global scroll style/visibility so the
//! Appearance demo's live tweaks apply here too.
//!
//! The scroll offset (`vbar.offset`) is folded into
//! [`TextArea::content_top_y`], so all existing geometry (paint, hit-test,
//! cursor overlay) inherits scrolling for free. `0` shows the first line;
//! larger offsets reveal lower lines.
//!
//! The vertical bar is a *floating overlay* (it does not shrink the text wrap
//! width). This matches the default [`ScrollBarKind::Floating`] and avoids a
//! wrap/overflow feedback loop; the thin dormant bar rarely occludes glyphs and
//! grows only while hovered/dragged.

use super::*;
use crate::geometry::Point;
use crate::widgets::scroll_view::{
    current_scroll_style, current_scroll_visibility, ScrollBarStyle, ScrollBarVisibility,
};
use crate::widgets::scrollbar::{
    paint_prepared_scrollbar, ScrollbarGeometry, ScrollbarOrientation, DEFAULT_GRAB_MARGIN,
};

/// Outcome of routing a `MouseMove` through the scroll bar first.
pub(crate) enum ScrollMove {
    /// The bar thumb is being dragged; `bool` = offset changed this move.
    Dragging(bool),
    /// Not dragging; `bool` = the bar's hover state changed this move.
    Hover(bool),
}

impl TextArea {
    /// Inner content height (widget height minus vertical padding).
    pub(crate) fn inner_height(&self) -> f64 {
        (self.bounds.height - self.padding * 2.0).max(0.0)
    }

    /// Total wrapped-content height at the current line count.
    pub(crate) fn content_height(&self) -> f64 {
        self.cached_lines.len() as f64 * self.cached_line_h
    }

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
    /// Called each layout after the wrap cache is rebuilt.
    pub(crate) fn sync_scroll(&mut self) {
        self.vbar.enabled = true;
        self.vbar.content = self.content_height();
        self.vbar.clamp_offset(self.inner_height());
    }

    /// Vertical track geometry in widget-local Y-up coords. The bar hugs the
    /// right inner edge; the track spans the padded inner height.
    fn v_scrollbar_geometry(&self) -> ScrollbarGeometry {
        ScrollbarGeometry {
            orientation: ScrollbarOrientation::Vertical,
            track_start: self.padding,
            track_end: (self.bounds.height - self.padding).max(self.padding),
            cross_end: (self.bounds.width - 2.0).max(0.0),
            hit_margin: DEFAULT_GRAB_MARGIN,
        }
    }

    /// Scroll so the caret's visual line is fully visible. Re-wraps first (the
    /// cache may be dirty after an edit) so the line index and content height
    /// are current. Called after every cursor-affecting key.
    pub(crate) fn ensure_cursor_visible(&mut self) {
        let inner_w = self.inner_width();
        self.refresh_wrap(inner_w);
        let line_h = self.cached_line_h;
        if line_h <= 0.0 {
            return;
        }
        let inner_h = self.inner_height();
        let cursor = self.edit.borrow().cursor;
        let line = self.line_for_cursor(cursor);
        // Line `i` occupies content-space [i·h, (i+1)·h], measured downward from
        // the content top. The visible window is [offset, offset + inner_h].
        let top = line as f64 * line_h;
        let bottom = top + line_h;
        let mut off = self.vbar.offset;
        if top < off {
            off = top; // caret above the window → scroll up to its line
        } else if bottom > off + inner_h {
            off = bottom - inner_h; // caret below → scroll down just enough
        }
        self.vbar.offset = off.clamp(0.0, self.max_scroll_y());
    }

    /// Wheel handler. Positive `delta_y` means "see content above" (decrease
    /// offset), matching the [`Event::MouseWheel`](crate::event::Event) sign
    /// convention. Returns `true` when the offset actually moved.
    pub(crate) fn scroll_by_wheel(&mut self, delta_y: f64) -> bool {
        if self.max_scroll_y() <= 0.0 {
            return false;
        }
        self.vbar.scroll_by(-delta_y, self.inner_height())
    }

    /// Begin a thumb drag if `pos` is on the thumb. Returns `true` when a drag
    /// started (caller must then suppress text selection).
    pub(crate) fn scrollbar_begin_drag(&mut self, pos: Point) -> bool {
        if self.max_scroll_y() <= 0.0 {
            return false;
        }
        let geom = self.v_scrollbar_geometry();
        self.vbar
            .begin_drag(pos, self.inner_height(), self.scroll_style(), geom)
    }

    /// Route a `MouseMove` through the bar: continue an in-progress drag, or
    /// update hover state.
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

    /// End any thumb drag. Returns `true` if a drag was in progress.
    pub(crate) fn scrollbar_end_drag(&mut self) -> bool {
        let was = self.vbar.dragging;
        self.vbar.dragging = false;
        was
    }

    /// Paint the vertical bar (thumb + track) using the global scroll style, so
    /// it looks identical to `ScrollView`'s bars.
    pub(crate) fn paint_scrollbar(&mut self, ctx: &mut dyn DrawCtx) {
        let vp = self.inner_height();
        let style = self.scroll_style();
        let vis = self.scroll_visibility();
        let geom = self.v_scrollbar_geometry();
        if let Some(bar) = self.vbar.prepare_paint(vp, style, vis, geom) {
            paint_prepared_scrollbar(ctx, bar);
        }
    }

    /// `true` while the bar has a running hover/visibility animation, so the
    /// widget keeps requesting frames until it settles.
    pub(crate) fn scrollbar_animating(&self) -> bool {
        self.vbar.animation_active()
    }
}
