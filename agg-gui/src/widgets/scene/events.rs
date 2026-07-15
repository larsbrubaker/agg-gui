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

use super::{Scene, DBL_CLICK_MS, ZOOM_SENSITIVITY};
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
            let delta = Point::new(pos.x - self.pan_last.x, pos.y - self.pan_last.y);
            self.transform.pan(delta);
            self.pan_last = pos;
            self.user_interacted = true;
            self.publish_scene_rect();
            crate::animation::request_draw();
            return EventResult::Consumed;
        }

        let content_local = self.to_content_local(pos);
        let new_hit = hit_test_subtree(self.content.as_ref(), content_local);

        // Clear hover on the previously-hovered path when the target changes.
        if new_hit != self.inner_hovered {
            if let Some(old) = self.inner_hovered.take() {
                let clear = Point::new(-1.0, -1.0);
                self.route_to_content(&old, &Event::MouseMove { pos: clear }, clear);
            }
            self.inner_hovered = new_hit.clone();
        }

        // A captured child keeps receiving real positions (drag outside bounds).
        if let Some(path) = self.inner_captured.clone() {
            self.route_to_content(&path, &Event::MouseMove { pos }, content_local);
            return EventResult::Consumed;
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
            if self.route_to_content(&path, event, content_local) == EventResult::Consumed {
                self.inner_captured = Some(path);
                return EventResult::Consumed;
            }
        }

        match button {
            MouseButton::Left => {
                let now = Instant::now();
                let is_double = self
                    .last_bg_click
                    .map(|t| now.duration_since(t).as_millis() < DBL_CLICK_MS)
                    .unwrap_or(false);
                if is_double {
                    self.last_bg_click = None;
                    self.reset_view();
                    return EventResult::Consumed;
                }
                self.last_bg_click = Some(now);
                self.begin_pan(pos);
                EventResult::Consumed
            }
            MouseButton::Middle => {
                self.begin_pan(pos);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn on_mouse_up(&mut self, pos: Point, event: &Event) -> EventResult {
        if self.panning {
            self.panning = false;
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

    fn begin_pan(&mut self, pos: Point) {
        self.panning = true;
        self.pan_last = pos;
    }
}
