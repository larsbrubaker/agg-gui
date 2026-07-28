//! Background-gesture handling for [`Scene`](super::Scene).
//!
//! `Scene` receives events in its own bottom-left-origin Y-up screen space
//! (the `App` has already translated ancestor offsets away).  The hosted
//! content is a first-class framework child, so pointer input reaches it
//! through the normal leaf→root dispatch — mapped through the Scene's
//! [`child_transform`](crate::widget::Widget::child_transform).  This module
//! therefore handles only what happens on the **empty background**: pan (left
//! or middle drag), zoom (wheel, anchored on the cursor), and double-click
//! reset.
//!
//! # Bubble + capture model
//!
//! The Scene sees a pointer event exactly when no hosted child consumed it —
//! the framework bubbles the press up to the Scene only after the content
//! declined it.  A press on empty canvas therefore reaches [`on_mouse_down`]
//! and starts a pan; a press on a hosted button is consumed by the button and
//! the Scene never sees it.  Once a pan starts, the Scene reports
//! [`claims_pointer_exclusively`](crate::widget::Widget::claims_pointer_exclusively)
//! so the `App` captures the Scene itself and every subsequent move lands here
//! regardless of what the cursor sweeps over.
//!
//! [`on_mouse_down`]: Scene::on_mouse_down

use super::{Scene, DBL_CLICK_MS, MAX_CLICK_DIST, ZOOM_SENSITIVITY};
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::Point;
use web_time::Instant;

impl Scene {
    /// Entry point called from `Widget::on_event`.
    pub(super) fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => self.on_mouse_move(*pos),
            Event::MouseDown { pos, button, .. } => self.on_mouse_down(*pos, *button),
            Event::MouseUp { pos, .. } => self.on_mouse_up(*pos),
            Event::MouseWheel { pos, delta_y, .. } => self.on_wheel(*pos, *delta_y),
            _ => EventResult::Ignored,
        }
    }

    fn on_mouse_move(&mut self, pos: Point) -> EventResult {
        // The Scene only acts on moves while it owns an active background pan;
        // hover/drag of hosted widgets is handled by the framework dispatching
        // directly to the content child.
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
        EventResult::Ignored
    }

    pub(super) fn on_mouse_down(&mut self, pos: Point, button: MouseButton) -> EventResult {
        // Reaching here means the content declined the press (it bubbled up),
        // so this is a background gesture.
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

    fn on_mouse_up(&mut self, _pos: Point) -> EventResult {
        if self.panning {
            self.panning = false;
            // Click vs drag is decided here, at release: only a left-button
            // press-release pair that never left the click tolerance counts
            // as a genuine background click for double-click reset.  A drag
            // (pan) clears any pending click so pan-then-press can never
            // fire an unintended reset.
            if self.pan_is_left && !self.pan_moved {
                let now = Instant::now();
                // A double-click needs two background clicks that are both
                // recent AND *consecutive* — no other press in between.  The
                // second condition is what the pointer-press epoch buys us:
                // a click on a hosted child bumps the epoch (the App counts
                // it) even though the child consumes it and the Scene never
                // sees it, so `bg-click → child-click → bg-click` reads as a
                // gap of two and correctly does not reset.
                let in_time = self
                    .last_bg_click
                    .map(|t| now.duration_since(t).as_millis() < DBL_CLICK_MS)
                    .unwrap_or(false);
                let consecutive =
                    self.last_bg_click_epoch == Some(self.pan_press_epoch.wrapping_sub(1));
                if in_time && consecutive {
                    self.last_bg_click = None;
                    self.last_bg_click_epoch = None;
                    self.reset_view();
                } else {
                    self.last_bg_click = Some(now);
                    self.last_bg_click_epoch = Some(self.pan_press_epoch);
                }
            } else {
                self.last_bg_click = None;
                self.last_bg_click_epoch = None;
            }
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    fn on_wheel(&mut self, pos: Point, delta_y: f64) -> EventResult {
        // The wheel zooms the scene, anchored on the cursor.  Under the
        // framework's leaf→root bubble the Scene receives the wheel only when
        // no hosted child consumed it first — so ordinary content (buttons,
        // labels, fields, painted shapes) zooms as expected, but a *scrollable*
        // hosted child would scroll instead.
        //
        // Deviation from egui: egui's Scene always zooms on scroll regardless
        // of what sits under the cursor; we deliver deepest-first.  Left as-is
        // deliberately — a scrollable-inside-a-Scene case isn't in use.  If one
        // appears, revisit with a pre-dispatch wheel interception rather than
        // baking special-casing in here.
        //
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
        // Snapshot the App's per-press counter for this background press so a
        // release can tell whether the previous background click was the
        // immediately-preceding press (a real double-click) or whether a
        // hosted-child click happened in between.
        self.pan_press_epoch = crate::animation::pointer_press_epoch();
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
        // Mirror the App contract: every pointer press routed to the tree
        // bumps the press epoch (see `App::on_mouse_down`).  These unit tests
        // drive `Scene::on_event` directly, so they must bump it themselves for
        // the consecutive-press double-click gate to see distinct presses.
        crate::animation::bump_pointer_press_epoch();
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
