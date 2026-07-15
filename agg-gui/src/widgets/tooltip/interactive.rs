//! Interactive-tooltip behaviour for [`super::Tooltip`].
//!
//! An interactive tip is a real floating UI surface hosting a child widget
//! tree (`Tooltip::content`). Unlike the lightweight text queue in
//! [`super::render`], it is hit-testable: the pointer can move off the anchor
//! and onto the tip without dismissing it, hyperlinks inside it receive clicks,
//! and a tooltip-bearing widget inside it shows its own (nested) tip.
//!
//! Coordinate model — mirrors `ComboBox`'s global-overlay popup:
//! * The tip panel is positioned in the Tooltip's LOCAL space (relative to the
//!   anchored widget's origin). `paint_global_overlay` runs with the ctx CTM at
//!   that origin, so painting and hit-testing share one frame.
//! * `hit_test_global_overlay` receives the pointer in that same local space,
//!   so it compares directly against the stored panel rect.
//!
//! Nesting: the content subtree is painted with the normal `paint_subtree`
//! walk, so any lightweight [`Tooltip`](super::Tooltip) inside it submits to
//! the global tooltip queue. That queue is drained a second time after the
//! global-overlay pass (`keyboard_scroll::paint_lifted_tree`) so the nested tip
//! renders above the parent tip. Nested tips close automatically because the
//! parent stops forwarding pointer events (and clears content hover) the moment
//! it closes.

use std::time::Duration;
use web_time::Instant;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key};
use crate::geometry::{Point, Rect, Size};
use crate::widget::{dispatch_event_dyn, hit_test_subtree, paint_subtree, Widget};

use super::render::current_tooltip_viewport;
use super::{
    Tooltip, SCREEN_MARGIN, TOOLTIP_GAP, TOOLTIP_INITIAL_DELAY, TOOLTIP_PAD_X, TOOLTIP_PAD_Y,
};

/// Grace period the interactive tip stays open after the pointer leaves both
/// the anchor and the tip, so crossing the small gap between them (or a
/// momentary pointer jump) does not flicker it closed. Matches egui's feel.
pub(super) const TOOLTIP_CLOSE_GRACE: Duration = Duration::from_millis(250);

/// Maximum width for interactive tip content before it must wrap.
const INTERACTIVE_MAX_WIDTH: f64 = 320.0;

impl Tooltip {
    /// Advance the open/close state machine. Called once per overlay paint.
    ///
    /// Opens after [`TOOLTIP_INITIAL_DELAY`] of anchor hover; stays open while
    /// the pointer is over the anchor or the tip; closes once it has left both
    /// for [`TOOLTIP_CLOSE_GRACE`]. Schedules wall-clock wakeups so the delayed
    /// transitions happen without continuous repainting.
    pub(super) fn update_interactive_state(&mut self) {
        if !self.tip_open {
            if self.hovered {
                if let Some(started) = self.hover_started_at {
                    let elapsed = started.elapsed();
                    if elapsed >= TOOLTIP_INITIAL_DELAY {
                        self.tip_open = true;
                        self.close_requested_at = None;
                        crate::animation::request_draw();
                    } else {
                        crate::animation::request_draw_after(TOOLTIP_INITIAL_DELAY - elapsed);
                    }
                }
            }
            return;
        }

        if self.hovered || self.tip_hovered {
            self.close_requested_at = None;
            return;
        }

        match self.close_requested_at {
            Some(since) => {
                let elapsed = since.elapsed();
                if elapsed >= TOOLTIP_CLOSE_GRACE {
                    self.close_tip();
                    crate::animation::request_draw();
                } else {
                    crate::animation::request_draw_after(TOOLTIP_CLOSE_GRACE - elapsed);
                }
            }
            None => {
                self.close_requested_at = Some(Instant::now());
                crate::animation::request_draw_after(TOOLTIP_CLOSE_GRACE);
            }
        }
    }

    /// Reset all interactive open-state and clear any nested content hover.
    pub(super) fn close_tip(&mut self) {
        self.tip_open = false;
        self.tip_hovered = false;
        self.close_requested_at = None;
        self.tip_panel_local = None;
        self.clear_content_hover();
    }

    /// Lay out `content` at its natural size and record it.
    pub(super) fn layout_content(&mut self) -> Size {
        // A large-but-finite height keeps column layouts well-behaved.
        let avail = Size::new(INTERACTIVE_MAX_WIDTH, 4096.0);
        if let Some(content) = self.content.as_mut() {
            let s = content.layout(avail);
            content.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
            self.content_size = s;
            s
        } else {
            Size::new(0.0, 0.0)
        }
    }

    /// Paint the interactive tip surface (panel + content) when open.
    ///
    /// The ctx CTM is at this widget's local origin (the anchor's bottom-left
    /// in world space). Geometry is computed in local coords; viewport-edge
    /// clamping converts through the logical world origin from `root_transform`.
    pub(super) fn paint_interactive_tip(&mut self, ctx: &mut dyn DrawCtx) {
        self.update_interactive_state();
        if !self.tip_open || self.content.is_none() {
            return;
        }

        let inner = self.layout_content();
        let panel_w = inner.width + TOOLTIP_PAD_X * 2.0;
        let panel_h = inner.height + TOOLTIP_PAD_Y * 2.0;

        // Below the anchor: in Y-up coords the anchor's bottom edge is y=0, so
        // "below" means negative y. Left edge aligned with the anchor.
        let mut panel_x = 0.0_f64;
        let mut panel_bottom = -TOOLTIP_GAP - panel_h;

        // Viewport-edge avoidance in logical world coords. `root_transform`
        // includes the outer device scale; strip it (as ComboBox does) so
        // offsets stay in logical units.
        let scale = crate::device_scale::device_scale().max(1e-6);
        let mut ox = 0.0;
        let mut oy = 0.0;
        ctx.root_transform().transform(&mut ox, &mut oy);
        let ox = ox / scale;
        let oy = oy / scale;
        let viewport = current_tooltip_viewport();
        if viewport.width > 0.0 && viewport.height > 0.0 {
            // Flip above the anchor when there is no room below.
            if oy + panel_bottom < SCREEN_MARGIN {
                panel_bottom = self.bounds.height + TOOLTIP_GAP;
            }
            // Shift left so the right edge stays on-screen.
            let overflow = (ox + panel_x + panel_w) - (viewport.width - SCREEN_MARGIN);
            if overflow > 0.0 {
                panel_x -= overflow;
            }
            // Never let the left edge cross the margin.
            let underflow = SCREEN_MARGIN - (ox + panel_x);
            if underflow > 0.0 {
                panel_x += underflow;
            }
        }

        self.tip_panel_local = Some(Rect::new(panel_x, panel_bottom, panel_w, panel_h));
        self.content_origin_local =
            Point::new(panel_x + TOOLTIP_PAD_X, panel_bottom + TOOLTIP_PAD_Y);

        let v = ctx.visuals();

        // Drop shadow.
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.20));
        ctx.begin_path();
        ctx.rounded_rect(panel_x + 1.0, panel_bottom - 1.0, panel_w, panel_h, 5.0);
        ctx.fill();

        // Panel fill + border.
        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rounded_rect(panel_x, panel_bottom, panel_w, panel_h, 5.0);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(panel_x, panel_bottom, panel_w, panel_h, 5.0);
        ctx.stroke();

        // Content subtree. paint_subtree also runs its `paint_overlay`, which
        // is how a nested lightweight Tooltip submits itself to the queue.
        let origin = self.content_origin_local;
        if let Some(content) = self.content.as_mut() {
            ctx.save();
            ctx.translate(origin.x, origin.y);
            paint_subtree(content.as_mut(), ctx);
            ctx.restore();
        }
    }

    /// Whether `local_pos` (Tooltip-local coords) is over the open tip panel.
    pub(super) fn interactive_hit(&self, local_pos: Point) -> bool {
        self.tip_open
            && self
                .tip_panel_local
                .map(|r| r.contains(local_pos))
                .unwrap_or(false)
    }

    /// Escape closes the open interactive tip (and any nested tip with it).
    pub(super) fn interactive_unconsumed_key(&mut self, key: &Key) -> EventResult {
        if self.tip_open && matches!(key, Key::Escape) {
            self.close_tip();
            crate::animation::request_draw();
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    /// Interactive-mode event handling: track anchor/tip hover, forward
    /// pointer events into the content subtree, and manage the open latch.
    pub(super) fn on_interactive_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                self.tip_hovered = self.interactive_hit(*pos);
                self.cursor = *pos;

                if self.hovered && !was {
                    self.hover_started_at = Some(Instant::now());
                    crate::animation::request_draw_after(TOOLTIP_INITIAL_DELAY);
                }
                if self.hovered || self.tip_hovered {
                    self.close_requested_at = None;
                } else if self.tip_open && self.close_requested_at.is_none() {
                    self.close_requested_at = Some(Instant::now());
                    crate::animation::request_draw_after(TOOLTIP_CLOSE_GRACE);
                }
                crate::animation::request_draw();

                if self.tip_open && self.tip_hovered {
                    let cpos = Point::new(
                        pos.x - self.content_origin_local.x,
                        pos.y - self.content_origin_local.y,
                    );
                    self.forward_move_to_content(cpos);
                } else {
                    self.clear_content_hover();
                }

                // The anchor child still tracks its own hover state.
                self.children
                    .first_mut()
                    .map(|child| child.on_event(event))
                    .unwrap_or(EventResult::Ignored)
            }
            Event::MouseDown { .. } | Event::MouseUp { .. } => {
                if self.tip_open && self.interactive_hit(pos_of(event)) {
                    let p = pos_of(event);
                    let cpos = Point::new(
                        p.x - self.content_origin_local.x,
                        p.y - self.content_origin_local.y,
                    );
                    return self.forward_click_to_content(event, cpos);
                }
                self.children
                    .first_mut()
                    .map(|child| child.on_event(event))
                    .unwrap_or(EventResult::Ignored)
            }
            Event::MouseWheel { .. } => {
                // Scrolling dismisses the tip (matches lightweight behaviour and
                // egui's scroll-test expectation).
                self.hovered = false;
                self.hover_started_at = None;
                self.close_tip();
                crate::animation::request_draw();
                self.children
                    .first_mut()
                    .map(|child| child.on_event(event))
                    .unwrap_or(EventResult::Ignored)
            }
            _ => self
                .children
                .first_mut()
                .map(|child| child.on_event(event))
                .unwrap_or(EventResult::Ignored),
        }
    }

    /// Route a `MouseMove` (in content-local coords) into the content subtree,
    /// clearing the previously-hovered descendant when the target changes so a
    /// nested tooltip resets as the pointer moves off its anchor.
    fn forward_move_to_content(&mut self, cpos: Point) {
        let Some(mut content) = self.content.take() else {
            return;
        };
        let new_path = hit_test_subtree(content.as_ref(), cpos);
        if new_path != self.last_content_path {
            if let Some(old) = self.last_content_path.take() {
                let clear = Event::MouseMove {
                    pos: Point::new(-1.0, -1.0),
                };
                dispatch_event_dyn(content.as_mut(), &old, &clear, Point::new(-1.0, -1.0));
            }
            self.last_content_path = new_path.clone();
        }
        if let Some(path) = new_path {
            let ev = Event::MouseMove { pos: cpos };
            dispatch_event_dyn(content.as_mut(), &path, &ev, cpos);
        }
        self.content = Some(content);
    }

    /// Route a press/release (in content-local coords) into the content subtree
    /// so hyperlinks and buttons inside the tip are clickable.
    fn forward_click_to_content(&mut self, event: &Event, cpos: Point) -> EventResult {
        let Some(mut content) = self.content.take() else {
            return EventResult::Ignored;
        };
        let result = match hit_test_subtree(content.as_ref(), cpos) {
            Some(path) => dispatch_event_dyn(content.as_mut(), &path, event, cpos),
            None => EventResult::Ignored,
        };
        self.content = Some(content);
        result
    }

    /// Send a hover-clear into the last content target so a nested tooltip (or
    /// hyperlink highlight) does not linger after the pointer leaves the tip.
    fn clear_content_hover(&mut self) {
        let Some(old) = self.last_content_path.take() else {
            return;
        };
        if let Some(mut content) = self.content.take() {
            let clear = Event::MouseMove {
                pos: Point::new(-1.0, -1.0),
            };
            dispatch_event_dyn(content.as_mut(), &old, &clear, Point::new(-1.0, -1.0));
            self.content = Some(content);
        }
    }
}

/// Extract the pointer position from a press/release event.
fn pos_of(event: &Event) -> Point {
    match event {
        Event::MouseDown { pos, .. } | Event::MouseUp { pos, .. } => *pos,
        _ => Point::ORIGIN,
    }
}
