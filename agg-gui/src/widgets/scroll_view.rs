//! `ScrollView` — vertical scrolling container.
//!
//! The single content child is laid out at its natural height (unconstrained),
//! then clipped to the visible area. A thin scrollbar on the right tracks
//! position and supports drag-to-scroll and mouse-wheel scrolling.
//!
//! # Clipping mechanism
//!
//! `paint()` installs a `clip_rect(0, 0, content_w, height)` in local space.
//! After `paint()` returns, `paint_subtree` translates to the child's
//! `(bounds.x, bounds.y)` — which shifts the content up or down according to
//! the scroll offset. Drawing outside the installed clip is discarded by AGG.


use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

const SCROLLBAR_W: f64 = 10.0;

pub struct ScrollView {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,  // always 0 or 1
    base: WidgetBase,
    scroll_offset: f64,
    content_height: f64,

    hovered_scrollbar: bool,
    dragging_scrollbar: bool,
    drag_start_y: f64,       // cursor Y when drag began (local Y-up)
    drag_start_offset: f64,  // scroll_offset when drag began
}

impl ScrollView {
    pub fn new(content: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![content],
            base: WidgetBase::new(),
            scroll_offset: 0.0,
            content_height: 0.0,
            hovered_scrollbar: false,
            dragging_scrollbar: false,
            drag_start_y: 0.0,
            drag_start_offset: 0.0,
        }
    }

    fn scrollbar_x(&self) -> f64 {
        self.bounds.width - SCROLLBAR_W
    }

    fn thumb_metrics(&self) -> Option<(f64, f64)> {
        let h = self.bounds.height;
        if self.content_height <= h { return None; }
        let ratio = h / self.content_height;
        let thumb_h = (h * ratio).max(20.0);
        let max_scroll = self.content_height - h;
        let track_h = h - thumb_h;
        let thumb_y = track_h * (1.0 - self.scroll_offset / max_scroll);
        Some((thumb_y, thumb_h))
    }

    fn max_scroll(&self) -> f64 {
        (self.content_height - self.bounds.height).max(0.0)
    }

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
        // Keep capturing during scrollbar drag even if cursor leaves bounds.
        if self.dragging_scrollbar { return true; }
        let b = self.bounds();
        local_pos.x >= 0.0 && local_pos.x <= b.width
            && local_pos.y >= 0.0 && local_pos.y <= b.height
    }

    fn layout(&mut self, available: Size) -> Size {
        let content_w = (available.width - SCROLLBAR_W).max(0.0);

        if let Some(child) = self.children.first_mut() {
            // Lay out content at full natural height (unconstrained vertically).
            let natural = child.layout(Size::new(content_w, f64::MAX / 2.0));
            self.content_height = natural.height;
        }

        // Clamp scroll offset (computed after content_height is updated).
        let max_s = (self.content_height - available.height).max(0.0);
        self.scroll_offset = self.scroll_offset.clamp(0.0, max_s);

        if let Some(child) = self.children.first_mut() {
            // Position child in Y-up space. child_y is the Y of the child's bottom-left.
            //   child_y = available.height - content_height + scroll_offset
            // scroll_offset = 0   → child_y = viewport_h - content_h (top of content at viewport top)
            // scroll_offset = max → child_y = 0 (bottom of content at viewport bottom)
            // Increasing scroll_offset raises child_y → content shifts UP → reveals lower items.
            let child_y = available.height - self.content_height + self.scroll_offset;
            child.set_bounds(Rect::new(0.0, child_y, content_w, self.content_height));
        }

        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let sb_x = self.scrollbar_x();

        // Clip content area — children drawn outside [0..h] are hidden.
        ctx.clip_rect(0.0, 0.0, sb_x, h);

        // Scrollbar track
        if self.content_height > h {
            let v = ctx.visuals();
            ctx.set_fill_color(v.scroll_track);
            ctx.begin_path();
            ctx.rect(sb_x, 0.0, SCROLLBAR_W, h);
            ctx.fill();

            // Scrollbar thumb
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

        let _ = w; // suppress unused warning
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        let sb_x = self.scrollbar_x();
        match event {
            Event::MouseWheel { delta_y, .. } => {
                // Convention: delta_y > 0 = user scrolled DOWN (wants to see content below).
                // Increasing scroll_offset moves content up → reveals lower items. ✓
                self.scroll_offset = (self.scroll_offset + delta_y * 40.0)
                    .clamp(0.0, self.max_scroll());
                EventResult::Consumed
            }
            Event::MouseMove { pos } => {
                // Only show scrollbar hover when content actually overflows.
                let scrollbar_visible = self.content_height > self.bounds.height;
                self.hovered_scrollbar = scrollbar_visible && pos.x >= sb_x;
                if self.dragging_scrollbar {
                    if let Some((_, thumb_h)) = self.thumb_metrics() {
                        let h = self.bounds.height;
                        let track_h = (h - thumb_h).max(1.0);
                        // pos.y increases upward; drag up = decrease scroll
                        let delta_y = self.drag_start_y - pos.y;
                        let scroll_per_px = self.max_scroll() / track_h;
                        self.scroll_offset = (self.drag_start_offset + delta_y * scroll_per_px)
                            .clamp(0.0, self.max_scroll());
                    }
                    EventResult::Consumed
                } else if self.hovered_scrollbar {
                    // Consume the hover so parent windows don't show a resize
                    // highlight for the right edge in the same region as the scrollbar.
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                if pos.x >= sb_x {
                    self.dragging_scrollbar = true;
                    self.drag_start_y = pos.y;
                    self.drag_start_offset = self.scroll_offset;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp { button: MouseButton::Left, .. } => {
                let was_dragging = self.dragging_scrollbar;
                self.dragging_scrollbar = false;
                if was_dragging { EventResult::Consumed } else { EventResult::Ignored }
            }
            _ => EventResult::Ignored,
        }
    }
}
