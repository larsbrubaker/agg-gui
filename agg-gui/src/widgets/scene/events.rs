//! Pointer-event routing for [`Scene`](super::Scene).
//!
//! `Scene` receives events in its own bottom-left-origin Y-up screen space
//! (the `App` has already translated ancestor offsets away). This module maps
//! those into scene coordinates and hand-routes them into the hosted content
//! subtree using the framework's own [`hit_test_subtree`] /
//! [`dispatch_event_dyn`] helpers, or handles them as pan/zoom/reset gestures
//! on the empty background.
//!
//! # Capture model
//!
//! When a child consumes a `MouseDown`, the outer `App` captures the *Scene*
//! (the deepest widget in the framework tree, since the content isn't a
//! framework child). The Scene remembers the internal path it dispatched to in
//! `inner_captured` and forwards subsequent moves/up there — so dragging a
//! slider inside the scene keeps working even when the cursor leaves the child.

use super::{Scene, DBL_CLICK_MS, MAX_CLICK_DIST, ZOOM_SENSITIVITY};
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::Point;
use crate::widget::{dispatch_event_dyn, hit_test_subtree};
use web_time::Instant;

impl Scene {
    /// Entry point called from `Widget::on_event`.
    pub(super) fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => self.on_mouse_move(*pos),
            Event::MouseDown { pos, button, .. } => self.on_mouse_down(*pos, *button, event),
            Event::MouseUp { pos, .. } => self.on_mouse_up(*pos, event),
            Event::MouseWheel { pos, delta_y, .. } => self.on_wheel(*pos, *delta_y),
            _ => EventResult::Ignored,
        }
    }

    /// Map a Scene-local screen point into the content subtree's local space
    /// (scene coordinates minus the content's own origin).
    fn to_content_local(&self, screen: Point) -> Point {
        let scene = self.transform.screen_to_scene(screen);
        let cb = self.content.bounds();
        Point::new(scene.x - cb.x, scene.y - cb.y)
    }

    /// Rebuild `event` with mouse positions replaced by `pos`. Only the mouse
    /// variants Scene routes carry a position; others pass through unchanged.
    fn event_with_pos(event: &Event, pos: Point) -> Event {
        match event {
            Event::MouseMove { .. } => Event::MouseMove { pos },
            Event::MouseDown {
                button, modifiers, ..
            } => Event::MouseDown {
                pos,
                button: *button,
                modifiers: *modifiers,
            },
            Event::MouseUp {
                button, modifiers, ..
            } => Event::MouseUp {
                pos,
                button: *button,
                modifiers: *modifiers,
            },
            other => other.clone(),
        }
    }

    /// Dispatch an event into the content subtree along `path`, translating its
    /// position into `content_local` first.
    fn route_to_content(
        &mut self,
        path: &[usize],
        event: &Event,
        content_local: Point,
    ) -> EventResult {
        let ev = Self::event_with_pos(event, content_local);
        dispatch_event_dyn(self.content.as_mut(), path, &ev, content_local)
    }

    fn on_mouse_move(&mut self, pos: Point) -> EventResult {
        // Active background pan takes priority over child hover.
        if self.panning {
            // Track whether the gesture left the click tolerance — a
            // press-release pair that moved is a drag, not a click, and must
            // not participate in double-click reset detection.
            if !self.pan_moved {
                let dx = pos.x - self.pan_press.x;
                let dy = pos.y - self.pan_press.y;
                if dx * dx + dy * dy > MAX_CLICK_DIST * MAX_CLICK_DIST {
                    self.pan_moved = true;
                }
            }
            let delta = Point::new(pos.x - self.pan_last.x, pos.y - self.pan_last.y);
            self.transform.pan(delta);
            self.pan_last = pos;
            self.user_interacted = true;
            self.publish_scene_rect();
            crate::animation::request_draw();
            return EventResult::Consumed;
        }

        let content_local = self.to_content_local(pos);

        // A captured child keeps receiving real positions (drag outside
        // bounds).  Checked BEFORE the hover diff so a mid-drag move can
        // never dispatch a spurious (-1,-1) hover-clear into the child that
        // owns the gesture.
        if let Some(path) = self.inner_captured.clone() {
            self.route_to_content(&path, &Event::MouseMove { pos }, content_local);
            return EventResult::Consumed;
        }

        let new_hit = hit_test_subtree(self.content.as_ref(), content_local);

        // Clear hover on the previously-hovered path when the target changes.
        if new_hit != self.inner_hovered {
            if let Some(old) = self.inner_hovered.take() {
                let clear = Point::new(-1.0, -1.0);
                self.route_to_content(&old, &Event::MouseMove { pos: clear }, clear);
            }
            self.inner_hovered = new_hit.clone();
        }

        if let Some(path) = new_hit {
            self.route_to_content(&path, &Event::MouseMove { pos }, content_local);
        }
        EventResult::Ignored
    }

    fn on_mouse_down(&mut self, pos: Point, button: MouseButton, event: &Event) -> EventResult {
        let content_local = self.to_content_local(pos);

        // Offer the press to the content first. A child that consumes it
        // captures the pointer; a non-interactive hit (label, background) falls
        // through to a pan gesture.
        if let Some(path) = hit_test_subtree(self.content.as_ref(), content_local) {
            if self.route_to_content(&path, event, content_local).is_consumed() {
                self.inner_captured = Some(path);
                // A child interaction breaks any pending background
                // double-click sequence (bg-click → child-click → bg-click
                // must not read as a double-click).
                self.last_bg_click = None;
                return EventResult::Consumed;
            }
        }

        match button {
            MouseButton::Left | MouseButton::Middle => {
                // Every background press starts a (potential) pan; whether it
                // was actually a *click* — and whether it completes a
                // double-click reset — is decided on release, once we know
                // the gesture didn't move (see `on_mouse_up`).
                self.begin_pan(pos, button == MouseButton::Left);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn on_mouse_up(&mut self, pos: Point, event: &Event) -> EventResult {
        if self.panning {
            self.panning = false;
            // Click vs drag is decided here, at release: only a left-button
            // press-release pair that never left the click tolerance counts
            // as a genuine background click for double-click reset.  A drag
            // (pan) clears any pending click so pan-then-press can never
            // fire an unintended reset.
            if self.pan_is_left && !self.pan_moved {
                let now = Instant::now();
                let is_double = self
                    .last_bg_click
                    .map(|t| now.duration_since(t).as_millis() < DBL_CLICK_MS)
                    .unwrap_or(false);
                if is_double {
                    self.last_bg_click = None;
                    self.reset_view();
                } else {
                    self.last_bg_click = Some(now);
                }
            } else {
                self.last_bg_click = None;
            }
            return EventResult::Consumed;
        }
        let content_local = self.to_content_local(pos);
        if let Some(path) = self.inner_captured.take() {
            return self.route_to_content(&path, event, content_local);
        }
        if let Some(path) = hit_test_subtree(self.content.as_ref(), content_local) {
            return self.route_to_content(&path, event, content_local);
        }
        EventResult::Ignored
    }

    fn on_wheel(&mut self, pos: Point, delta_y: f64) -> EventResult {
        // Wheel always zooms the scene (never scrolls a child) — matching egui.
        // Positive delta_y (wheel forward / scroll up) zooms in; the factor is
        // exponential so zoom-in and zoom-out are symmetric.
        let factor = (delta_y * ZOOM_SENSITIVITY).exp();
        let new_zoom = self.transform.zoom * factor;
        self.transform.zoom_at(pos, new_zoom, self.zoom_range);
        self.user_interacted = true;
        self.publish_scene_rect();
        crate::animation::request_draw();
        EventResult::Consumed
    }

    fn begin_pan(&mut self, pos: Point, is_left: bool) {
        self.panning = true;
        self.pan_last = pos;
        self.pan_press = pos;
        self.pan_moved = false;
        self.pan_is_left = is_left;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_ctx::DrawCtx;
    use crate::event::Modifiers;
    use crate::geometry::{Rect, Size};
    use crate::widget::Widget;

    /// Inert 100×100 content widget — hit-testable but never consumes events,
    /// so every press lands on the Scene background.
    struct InertContent {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
    }

    impl Widget for InertContent {
        fn type_name(&self) -> &'static str {
            "InertContent"
        }
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn set_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn children(&self) -> &[Box<dyn Widget>] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            &mut self.children
        }
        fn layout(&mut self, _available: Size) -> Size {
            self.bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
            Size::new(100.0, 100.0)
        }
        fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _event: &Event) -> EventResult {
            EventResult::Ignored
        }
    }

    fn scene() -> Scene {
        let mut s = Scene::new(Box::new(InertContent {
            bounds: Rect::default(),
            children: Vec::new(),
        }));
        s.layout(Size::new(400.0, 400.0));
        s
    }

    fn down(s: &mut Scene, x: f64, y: f64) {
        s.on_event(&Event::MouseDown {
            pos: Point::new(x, y),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
    }
    fn mv(s: &mut Scene, x: f64, y: f64) {
        s.on_event(&Event::MouseMove {
            pos: Point::new(x, y),
        });
    }
    fn up(s: &mut Scene, x: f64, y: f64) {
        s.on_event(&Event::MouseUp {
            pos: Point::new(x, y),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
    }

    #[test]
    fn pan_then_press_does_not_reset_view() {
        let mut s = scene();
        let fitted = s.transform();

        // Drag the background: press, move well beyond the click tolerance,
        // release.  This pans the view away from the fitted transform.
        down(&mut s, 200.0, 200.0);
        mv(&mut s, 250.0, 260.0);
        up(&mut s, 250.0, 260.0);
        let panned = s.transform();
        assert_ne!(panned, fitted, "drag should have panned the view");

        // Press again immediately (well inside the double-click window).
        // Before the click/drag distinction this falsely fired reset_view;
        // it must leave the panned transform untouched.
        down(&mut s, 250.0, 260.0);
        up(&mut s, 250.0, 260.0);
        assert_eq!(
            s.transform(),
            panned,
            "press after a drag must not reset the view"
        );
    }

    #[test]
    fn genuine_double_click_resets_view() {
        let mut s = scene();
        let fitted = s.transform();

        // Pan away so the reset is observable.
        down(&mut s, 200.0, 200.0);
        mv(&mut s, 260.0, 240.0);
        up(&mut s, 260.0, 240.0);
        assert_ne!(s.transform(), fitted);

        // Two genuine clicks (no motion) in quick succession → reset.  The
        // first click is preceded by a drag, which cleared the pending
        // click, so it arms the sequence and the second completes it.
        down(&mut s, 300.0, 300.0);
        up(&mut s, 300.0, 300.0);
        down(&mut s, 300.0, 300.0);
        up(&mut s, 300.0, 300.0);
        assert_eq!(
            s.transform(),
            fitted,
            "double-click on the background must reset to the fitted view"
        );
    }

    #[test]
    fn small_jitter_still_counts_as_click() {
        let mut s = scene();
        let fitted = s.transform();

        // Pan away first (also clears the pending-click state).
        down(&mut s, 200.0, 200.0);
        mv(&mut s, 150.0, 150.0);
        up(&mut s, 150.0, 150.0);
        let panned = s.transform();
        assert_ne!(panned, fitted);

        // Double-click with sub-tolerance jitter between press and release —
        // still a double-click, so the view resets.
        down(&mut s, 300.0, 300.0);
        mv(&mut s, 302.0, 301.0);
        up(&mut s, 302.0, 301.0);
        down(&mut s, 302.0, 301.0);
        mv(&mut s, 300.0, 300.0);
        up(&mut s, 300.0, 300.0);
        assert_eq!(s.transform(), fitted, "jittery double-click should reset");
    }
}
