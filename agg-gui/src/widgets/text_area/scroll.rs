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

/// Shared read-out of a [`TextArea`]'s scroll state for a *sibling* widget that
/// must mirror the editor's viewport (e.g. a line-number gutter) but lives
/// outside its subtree and so cannot see the internal scroll bar. Published
/// through [`TextArea::with_scroll_watch`].
///
/// The gutter's core problem is that the editor scrolls in *visual-row* pixel
/// space while it numbers *source* lines; with soft-wrapping the two diverge, so
/// a bare offset is not enough. `source_line_rows` bridges that: it maps each
/// source line to the visual row its number should sit on.
#[derive(Clone, Debug, Default)]
pub struct TextAreaScrollInfo {
    /// Vertical scroll offset in px from the top (0 = first row visible), the
    /// same value [`TextArea::scroll_offset`] reports. Refreshed on every offset
    /// change (wheel, drag, scroll-to-caret, relayout clamp).
    pub offset_px: f64,
    /// For each source line (paragraph), the index of its first wrapped visual
    /// row. `source_line_rows[i]` is where source line `i`'s number belongs;
    /// continuation rows of a wrapped line fall between consecutive entries and
    /// get no number. With no soft-wrapping this degenerates to `rows[i] == i`.
    /// Refreshed only when the wrap cache rebuilds (not per scroll).
    pub source_line_rows: Vec<usize>,
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

    /// Publish this widget's live scroll state into a shared
    /// [`TextAreaScrollInfo`] whenever it changes, so a *sibling* widget outside
    /// the editor's subtree can mirror the viewport — the Code Editor demo's
    /// line-number gutter uses it to keep numbers aligned with the scrolled,
    /// soft-wrapped text, which it otherwise cannot see. The offset is refreshed
    /// on every scroll and the source-line-to-row map on every re-wrap, so both
    /// are current before the next frame paints.
    pub fn with_scroll_watch(mut self, watch: Rc<RefCell<TextAreaScrollInfo>>) -> Self {
        self.scroll_watch = Some(watch);
        self.publish_scroll_offset();
        self.publish_scroll_rows();
        self
    }

    /// Number of whole visual lines that fit in the viewport — the distance
    /// PageUp/PageDown move the caret. Always at least 1 so the caret advances
    /// even in a viewport shorter than one line.
    pub(crate) fn page_lines(&self) -> usize {
        if self.cached_line_h <= 0.0 {
            return 1;
        }
        ((self.inner_height() / self.cached_line_h).floor() as usize).max(1)
    }

    fn scroll_style(&self) -> ScrollBarStyle {
        current_scroll_style()
    }
    fn scroll_visibility(&self) -> ScrollBarVisibility {
        current_scroll_visibility()
    }

    /// Mirror the current `vbar.offset` into the scroll-watch binding. Called
    /// from every site that moves the offset so a sibling widget (the
    /// line-number gutter) never paints a frame behind the text. See
    /// [`TextArea::with_scroll_watch`].
    fn publish_scroll_offset(&self) {
        if let Some(watch) = &self.scroll_watch {
            watch.borrow_mut().offset_px = self.vbar.offset;
        }
    }

    /// Recompute the source-line-to-visual-row map from the wrap cache and
    /// publish it. Called only on a real re-wrap (the `refresh_wrap` choke
    /// point), since wrapping only changes when the text or width does — not on
    /// scroll. Each source line (paragraph) spans the wrapped rows up to and
    /// including the one marked `hard_break`, so the first row of the next
    /// source line is one past each hard break; source line 0 always starts at
    /// row 0.
    pub(crate) fn publish_scroll_rows(&self) {
        let Some(watch) = &self.scroll_watch else {
            return;
        };
        let mut rows = Vec::with_capacity(self.cached_lines.len() + 1);
        rows.push(0);
        for (idx, line) in self.cached_lines.iter().enumerate() {
            if line.hard_break {
                rows.push(idx + 1);
            }
        }
        watch.borrow_mut().source_line_rows = rows;
    }

    /// Refresh the axis with the current content height and re-clamp the offset.
    /// Called each layout after the wrap cache is rebuilt.
    pub(crate) fn sync_scroll(&mut self) {
        self.vbar.enabled = true;
        self.vbar.content = self.content_height();
        self.vbar.clamp_offset(self.inner_height());
        self.publish_scroll_offset();
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
        self.publish_scroll_offset();
    }

    /// Wheel handler. Positive `delta_y` means "see content above" (decrease
    /// offset), matching the [`Event::MouseWheel`](crate::event::Event) sign
    /// convention. Returns `true` when the offset actually moved.
    pub(crate) fn scroll_by_wheel(&mut self, delta_y: f64) -> bool {
        if self.max_scroll_y() <= 0.0 {
            return false;
        }
        // Windows convention: one wheel notch scrolls 3 lines. `delta_y` arrives
        // in line-units (1.0 == one notch; the platform adapter pre-scales
        // trackpad pixel deltas into fractional line-units), so scaling by
        // 3 * line-height gives a crisp 3-line notch while keeping pixel-precise
        // trackpad panning smooth. This is more content-aware than ScrollView's
        // flat 40 px/line, which felt slow for the editors' larger line pitch.
        let line_h = if self.cached_line_h > 0.0 {
            self.cached_line_h
        } else {
            self.font_size * 1.35
        };
        let moved = self
            .vbar
            .scroll_by(-delta_y * 3.0 * line_h, self.inner_height());
        self.publish_scroll_offset();
        moved
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
            let moved = self.vbar.drag_to(pos, vp, style, geom);
            self.publish_scroll_offset();
            ScrollMove::Dragging(moved)
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
