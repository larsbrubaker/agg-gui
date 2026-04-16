//! `ScrollView` — vertical scrolling container with an egui-style floating scrollbar.
//!
//! The content child is laid out at its natural height (unconstrained) and clipped
//! to the visible area.  The scrollbar floats **on top of** the content (no width
//! is reserved) so the child always gets the full widget width.
//!
//! # Scrollbar behaviour (egui "floating" style)
//!
//! The bar lives at the right edge of the widget:
//! - **Dormant** — 2 px thin strip (`BAR_THIN_W`), no track background.
//! - **Hovered** — expands to 6 px (`BAR_FULL_W`), track background appears.
//! - **Dragging** — stays at full width, thumb uses active colour.
//!
//! Hover zone: rightmost `HOVER_ZONE_W` (10 px) of the widget.
//!
//! # Scrollbar geometry (Y-up)
//!
//! The track spans `[BAR_Y_MARGIN, height − BAR_Y_MARGIN]` along Y.
//! `thumb_y` is the **bottom** edge of the thumb in local space:
//! - `scroll_offset = 0`   → `thumb_y = track_h`  (thumb at visual top)
//! - `scroll_offset = max` → `thumb_y = 0`         (thumb at visual bottom)

use std::cell::Cell;
use std::rc::Rc;

use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

/// How the vertical scrollbar is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollBarVisibility {
    /// Scrollbar is painted whenever content overflows (even with no hover).
    AlwaysVisible,
    /// Scrollbar is painted only while the cursor is in the hover zone or a
    /// drag is in progress.  This is the default and matches egui's "floating"
    /// style.
    VisibleOnHover,
    /// Scrollbar is never painted — wheel/drag still works inside the widget
    /// but no visual indicator appears.
    AlwaysHidden,
}

impl Default for ScrollBarVisibility {
    fn default() -> Self { Self::VisibleOnHover }
}

// ── Egui-matching constants ────────────────────────────────────────────────────

/// Bar width when hovered / active.  Below this threshold the bar is not
/// rendered at all — the scrollbar is fully invisible in its dormant state.
const BAR_FULL_W: f64 = 10.0;
/// Top / bottom margin inside the track (shrinks effective track height).
const BAR_Y_MARGIN: f64 = 4.0;
/// Minimum thumb height in pixels.
const HANDLE_MIN_H: f64 = 24.0;
/// Pixels at the right edge that belong exclusively to the parent window's
/// resize handle.  The scrollbar is inset by this amount from the right edge
/// so the resize grip always remains grabbable.
const RIGHT_EDGE_GUARD: f64 = 4.0;
/// Extra grab margin to the left of the bar so the thumb is easy to grab
/// even when it is rendered thin.
const GRAB_MARGIN: f64 = 6.0;
/// Width of the hover-detection zone:  grab margin + full bar width.
const HOVER_ZONE_W: f64 = BAR_FULL_W + GRAB_MARGIN;

pub struct ScrollView {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,  // always 0 or 1
    base:     WidgetBase,

    scroll_offset:  f64,
    content_height: f64,

    /// Cursor is inside the hover zone (rightmost HOVER_ZONE_W px).
    hovered_bar: bool,
    /// Cursor is directly over the thumb (subset of hovered_bar).
    hovered_thumb: bool,
    /// Scrollbar drag in progress.
    dragging: bool,
    /// Pixel distance from thumb bottom edge to cursor at drag start (Y-up).
    drag_thumb_offset: f64,

    /// Whether the scrollbar should remain glued to the bottom as content grows.
    /// Tracks whether the offset was at max during the last layout so the user
    /// can still temporarily detach by scrolling up.
    stick_to_bottom: bool,
    was_at_bottom:   bool,

    /// How to render the scrollbar.  Event handling (wheel, drag) still works
    /// in every mode — only the paint changes.
    bar_visibility: ScrollBarVisibility,

    /// Optional external scroll-offset cell.  When set, `layout` reads the
    /// requested offset from the cell, clamps it, and writes the clamped
    /// value back.  Lets the surrounding UI programmatically scroll.
    offset_cell:     Option<Rc<Cell<f64>>>,
    /// Optional output cell — written each layout with the maximum scroll
    /// distance (content_height − viewport_height).  Useful for demos that
    /// display "offset / max" readouts.
    max_scroll_cell: Option<Rc<Cell<f64>>>,
    /// Optional input cell that drives the bar-visibility mode at runtime.
    visibility_cell: Option<Rc<Cell<ScrollBarVisibility>>>,
}

impl ScrollView {
    pub fn new(content: Box<dyn Widget>) -> Self {
        Self {
            bounds:            Rect::default(),
            children:          vec![content],
            base:              WidgetBase::new(),
            scroll_offset:     0.0,
            content_height:    0.0,
            hovered_bar:       false,
            hovered_thumb:     false,
            dragging:          false,
            drag_thumb_offset: 0.0,
            stick_to_bottom:   false,
            was_at_bottom:     false,
            bar_visibility:    ScrollBarVisibility::default(),
            offset_cell:       None,
            max_scroll_cell:   None,
            visibility_cell:   None,
        }
    }

    // ── Public scroll API ─────────────────────────────────────────────────────

    /// Current vertical scroll offset in pixels (0 at top, `max_scroll` at bottom).
    pub fn scroll_offset(&self) -> f64 { self.scroll_offset }

    /// Set the scroll offset; clamped to the valid range at the next layout.
    /// If an external offset cell is bound, that cell is updated too.
    pub fn set_scroll_offset(&mut self, offset: f64) {
        self.scroll_offset = offset;
        if let Some(c) = &self.offset_cell { c.set(offset); }
    }

    /// Maximum valid scroll offset for the current content.
    pub fn max_scroll_value(&self) -> f64 { self.max_scroll() }

    /// Bind an external cell that drives (and reflects) the scroll offset.
    pub fn with_offset_cell(mut self, cell: Rc<Cell<f64>>) -> Self {
        self.offset_cell = Some(cell);
        self
    }

    /// Bind an external cell that receives the computed `max_scroll` each layout.
    pub fn with_max_scroll_cell(mut self, cell: Rc<Cell<f64>>) -> Self {
        self.max_scroll_cell = Some(cell);
        self
    }

    /// Keep the scrollbar glued to the bottom as content grows (while the
    /// user hasn't scrolled away from the end).  Off by default.
    pub fn with_stick_to_bottom(mut self, stick: bool) -> Self {
        self.stick_to_bottom = stick;
        self
    }

    /// Control when the scrollbar thumb/track is painted.
    pub fn with_bar_visibility(mut self, v: ScrollBarVisibility) -> Self {
        self.bar_visibility = v;
        self
    }

    pub fn set_bar_visibility(&mut self, v: ScrollBarVisibility) {
        self.bar_visibility = v;
    }

    /// Bind an external cell that drives the bar-visibility mode at runtime.
    /// The cell is polled on every layout.
    pub fn with_bar_visibility_cell(mut self, cell: Rc<Cell<ScrollBarVisibility>>) -> Self {
        self.visibility_cell = Some(cell);
        self
    }

    // ── Geometry helpers ──────────────────────────────────────────────────────

    fn max_scroll(&self) -> f64 {
        (self.content_height - self.bounds.height).max(0.0)
    }

    /// Track y-range: `[BAR_Y_MARGIN, height − BAR_Y_MARGIN]`.
    fn track_range(&self) -> (f64, f64) {
        let lo = BAR_Y_MARGIN;
        let hi = (self.bounds.height - BAR_Y_MARGIN).max(lo);
        (lo, hi)
    }

    /// Compute `(thumb_y, thumb_h)` in local Y-up coordinates.
    /// `thumb_y` is the bottom edge of the thumb.
    /// Returns `None` when content fits without scrolling.
    fn thumb_metrics(&self) -> Option<(f64, f64)> {
        let h = self.bounds.height;
        if self.content_height <= h { return None; }

        let (track_lo, track_hi) = self.track_range();
        let track_h = track_hi - track_lo;

        let ratio   = h / self.content_height;
        let thumb_h = (track_h * ratio).max(HANDLE_MIN_H);
        let travel  = (track_h - thumb_h).max(0.0);
        let max_s   = self.max_scroll();

        // Y-up: offset=0 → thumb at top (thumb_y = track_hi − thumb_h + thumb_h = track_hi... wait)
        // track_lo is the bottom of the track space, track_hi is the top.
        // offset=0 → thumb_y (bottom) = track_hi - thumb_h  (thumb sits at top of track)
        // offset=max → thumb_y (bottom) = track_lo           (thumb sits at bottom of track)
        let thumb_y = if max_s > 0.0 {
            track_lo + travel * (1.0 - self.scroll_offset / max_s)
        } else {
            track_lo + travel
        };
        Some((thumb_y, thumb_h))
    }

    /// Right edge of the bar in local X (exclusive).  Bar sits left of the
    /// resize guard.
    fn bar_right(&self) -> f64 {
        self.bounds.width - RIGHT_EDGE_GUARD
    }

    /// True when `pos` lands on the thumb rectangle.  Horizontal hit area
    /// is extended leftward by `GRAB_MARGIN` so a thin dormant bar is still
    /// easy to grab.
    fn pos_on_thumb(&self, pos: Point) -> bool {
        let bar_right = self.bar_right();
        let hit_left  = bar_right - BAR_FULL_W - GRAB_MARGIN;
        if pos.x < hit_left || pos.x >= bar_right { return false; }
        if let Some((thumb_y, thumb_h)) = self.thumb_metrics() {
            pos.y >= thumb_y && pos.y <= thumb_y + thumb_h
        } else {
            false
        }
    }

    /// True when `pos` is in the scrollbar hover-detection zone.
    ///
    /// The rightmost `RIGHT_EDGE_GUARD` pixels are excluded so the parent
    /// window's resize handle always remains grabbable without interference.
    fn pos_in_hover_zone(&self, pos: Point) -> bool {
        let bar_right = self.bar_right();
        pos.x >= bar_right - HOVER_ZONE_W && pos.x < bar_right
    }

    /// Clamp and snap offset to integer pixels.
    fn clamp_offset(&self, raw: f64) -> f64 {
        raw.clamp(0.0, self.max_scroll()).round()
    }

    // ── Layout property forwarding ────────────────────────────────────────────

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }
}

impl Widget for ScrollView {
    fn type_name(&self) -> &'static str { "ScrollView" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn hit_test(&self, local_pos: Point) -> bool {
        if self.dragging { return true; }
        let b = self.bounds();
        local_pos.x >= 0.0 && local_pos.x <= b.width
            && local_pos.y >= 0.0 && local_pos.y <= b.height
    }

    /// The scrollbar floats above children, so when the cursor is in the
    /// scrollbar hover zone we must claim the event before any child (e.g.
    /// a full-width list row) can consume it.
    fn claims_pointer_exclusively(&self, local_pos: Point) -> bool {
        if self.dragging { return true; }
        if self.content_height <= self.bounds.height { return false; }
        self.pos_in_hover_zone(local_pos)
    }

    fn layout(&mut self, available: Size) -> Size {
        // Child gets the full widget width — bar floats on top.
        let content_w = available.width;

        // Pull requested offset from external cell before we clamp, so the
        // surrounding UI can drive the scroll position.
        if let Some(c) = &self.offset_cell {
            self.scroll_offset = c.get();
        }
        if let Some(c) = &self.visibility_cell {
            self.bar_visibility = c.get();
        }

        self.bounds = Rect::new(0.0, 0.0, content_w, available.height);
        if let Some(child) = self.children.first_mut() {
            let natural = child.layout(Size::new(content_w, f64::MAX / 2.0));
            self.content_height = natural.height;
        }

        // Apply stick-to-bottom AFTER we know the new content_height: if we
        // were at the end on the previous frame, follow the end as it moves.
        if self.stick_to_bottom && self.was_at_bottom {
            self.scroll_offset = self.max_scroll();
        }
        self.scroll_offset = self.clamp_offset(self.scroll_offset);
        self.was_at_bottom = (self.max_scroll() - self.scroll_offset).abs() < 0.5;

        // Publish back to the external cells.
        if let Some(c) = &self.offset_cell     { c.set(self.scroll_offset); }
        if let Some(c) = &self.max_scroll_cell { c.set(self.max_scroll());  }

        if let Some(child) = self.children.first_mut() {
            let child_y = available.height - self.content_height + self.scroll_offset;
            child.set_bounds(Rect::new(0.0, child_y.round(), content_w, self.content_height));
        }

        available
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        // Background drawn by parent / Window. Scrollbar is in paint_overlay.
    }

    /// Draw the floating scrollbar on top of all children.
    ///
    /// Because `paint_overlay` runs after `ctx.restore()` (which lifts the
    /// children clip), the bar is never clipped by `clip_children_rect`.
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let h = self.bounds.height;
        if self.content_height <= h { return; }

        // Decide whether to draw based on visibility mode.
        let is_hover = self.hovered_bar || self.dragging;
        let paint = match self.bar_visibility {
            ScrollBarVisibility::AlwaysHidden  => false,
            ScrollBarVisibility::AlwaysVisible => true,
            ScrollBarVisibility::VisibleOnHover => is_hover,
        };
        if !paint { return; }

        let Some((thumb_y, thumb_h)) = self.thumb_metrics() else { return };

        let v     = ctx.visuals();
        let bar_w = BAR_FULL_W;
        let bar_x = self.bar_right() - bar_w;
        let r     = bar_w * 0.5;

        // Track background — only drawn while visible.
        let (track_lo, track_hi) = self.track_range();
        ctx.set_fill_color(v.scroll_track);
        ctx.begin_path();
        ctx.rounded_rect(bar_x, track_lo, bar_w, track_hi - track_lo, r);
        ctx.fill();

        // Thumb.
        let thumb_color = if self.dragging {
            v.scroll_thumb_dragging
        } else if self.hovered_thumb {
            v.scroll_thumb_hovered
        } else {
            v.scroll_thumb
        };

        ctx.set_fill_color(thumb_color);
        ctx.begin_path();
        ctx.rounded_rect(bar_x, thumb_y, bar_w, thumb_h, r);
        ctx.fill();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            // ── Mouse wheel ───────────────────────────────────────────────────
            Event::MouseWheel { delta_y, .. } => {
                self.scroll_offset = self.clamp_offset(
                    self.scroll_offset + delta_y * 40.0,
                );
                // Manual scroll detaches from the sticky-bottom mode until the
                // user returns to the end.  If they land exactly at the end
                // again, re-enable sticking.
                self.was_at_bottom = (self.max_scroll() - self.scroll_offset).abs() < 0.5;
                if let Some(c) = &self.offset_cell { c.set(self.scroll_offset); }
                EventResult::Consumed
            }

            // ── Mouse move ────────────────────────────────────────────────────
            Event::MouseMove { pos } => {
                let scrollable = self.content_height > self.bounds.height;
                self.hovered_bar   = scrollable && self.pos_in_hover_zone(*pos);
                self.hovered_thumb = scrollable && self.pos_on_thumb(*pos);

                if self.dragging {
                    if let Some((_, thumb_h)) = self.thumb_metrics() {
                        let (track_lo, track_hi) = self.track_range();
                        let travel = (track_hi - track_lo - thumb_h).max(1.0);
                        // New bottom of thumb, clamped to track.
                        let new_thumb_y = (pos.y - self.drag_thumb_offset)
                            .clamp(track_lo, track_lo + travel);
                        let scroll_frac = 1.0 - (new_thumb_y - track_lo) / travel;
                        self.scroll_offset =
                            self.clamp_offset(scroll_frac * self.max_scroll());
                        self.was_at_bottom =
                            (self.max_scroll() - self.scroll_offset).abs() < 0.5;
                        if let Some(c) = &self.offset_cell { c.set(self.scroll_offset); }
                    }
                    return EventResult::Consumed;
                }

                // Return Ignored so the event bubbles to the parent Window, which
                // must be able to detect its resize edge even when the scrollbar
                // hover zone overlaps the window's right-edge resize zone.
                EventResult::Ignored
            }

            // ── Mouse down ────────────────────────────────────────────────────
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                if !self.pos_in_hover_zone(*pos) { return EventResult::Ignored; }

                if self.pos_on_thumb(*pos) {
                    // Drag: remember offset from thumb bottom to cursor.
                    let thumb_y = self.thumb_metrics().map(|(y, _)| y).unwrap_or(0.0);
                    self.dragging          = true;
                    self.drag_thumb_offset = pos.y - thumb_y;
                } else if self.content_height > self.bounds.height {
                    // Click on track: center thumb at click position.
                    if let Some((_, thumb_h)) = self.thumb_metrics() {
                        let (track_lo, track_hi) = self.track_range();
                        let travel = (track_hi - track_lo - thumb_h).max(1.0);
                        let new_thumb_y = (pos.y - thumb_h * 0.5)
                            .clamp(track_lo, track_lo + travel);
                        let scroll_frac = 1.0 - (new_thumb_y - track_lo) / travel;
                        self.scroll_offset =
                            self.clamp_offset(scroll_frac * self.max_scroll());
                    }
                }
                EventResult::Consumed
            }

            // ── Mouse up ──────────────────────────────────────────────────────
            Event::MouseUp { button: MouseButton::Left, .. } => {
                let was = self.dragging;
                self.dragging = false;
                if was { EventResult::Consumed } else { EventResult::Ignored }
            }

            _ => EventResult::Ignored,
        }
    }
}
