//! Pointer-input methods on [`App`] — mouse move/down/up plus the
//! modal-subtree hit extension they share. Split out of `app.rs`
//! (800-line guardrail); wheel and keyboard routing stay in `app.rs`.

use super::tree_paths::widget_at_path;
use crate::event::{Event, Modifiers, MouseButton};
use crate::geometry::Point;
use crate::widget::tree::{active_modal_path, dispatch_event, hit_test_subtree};
use crate::widget::tree_inspector::set_current_mouse_world;
use crate::widget::{App, Widget};

impl App {
    /// Extend the active modal widget's path by hit-testing inside its
    /// subtree at `pos_in_root`, so the modal's own children (buttons,
    /// fields, scroll views) receive pointer events. Falls back to the
    /// modal widget itself when nothing inside is hit (the scrim).
    pub(crate) fn extend_modal_path(&self, modal_path: &[usize], pos_in_root: Point) -> Vec<usize> {
        let mut widget: &dyn Widget = self.root.as_ref();
        let mut pos = pos_in_root;
        for &index in modal_path {
            let Some(child) = widget.children().get(index) else {
                return modal_path.to_vec();
            };
            let bounds = child.bounds();
            pos = Point::new(pos.x - bounds.x, pos.y - bounds.y);
            widget = child.as_ref();
        }
        let mut full = modal_path.to_vec();
        if let Some(sub_path) = hit_test_subtree(widget, pos) {
            full.extend(sub_path);
        }
        full
    }

    /// Mouse cursor moved. `screen_y` is Y-down physical pixels.
    pub fn on_mouse_move(&mut self, screen_x: f64, screen_y: f64) {
        // Reset cursor so the hovered widget can set it; Default if nothing sets it.
        crate::cursor::reset_cursor_icon();
        let screen = self.flip_y(screen_x, screen_y);
        if crate::widgets::on_screen_keyboard::handle_software_keyboard_mouse_move(screen) {
            self.drain_keyboard_synthetic_keys();
            return;
        }
        let pos = super::keyboard_scroll::lift_to_world(screen);
        set_current_mouse_world(pos);
        if let Some(path) = active_modal_path(self.root.as_ref()) {
            let path = self.extend_modal_path(&path, pos);
            let event = Event::MouseMove { pos };
            dispatch_event(&mut self.root, &path, &event, pos);
            self.hovered = Some(path);
            return;
        }
        self.dispatch_mouse_move(pos);
    }

    /// Mouse button pressed. `screen_y` is Y-down physical pixels.
    pub fn on_mouse_down(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        button: MouseButton,
        mods: Modifiers,
    ) {
        let screen = self.flip_y(screen_x, screen_y);
        // On-screen keyboard captures pointer events on its panel area
        // before anything in the tree gets a look. Returning here also
        // means the focused widget keeps focus (so the keyboard does
        // not dismiss itself by stealing focus on every key tap).
        if crate::widgets::on_screen_keyboard::handle_software_keyboard_mouse_down(
            screen, button, mods,
        ) {
            return;
        }
        let pos = super::keyboard_scroll::lift_to_world(screen);
        set_current_mouse_world(pos);
        let modal_path = active_modal_path(self.root.as_ref());
        let event = Event::MouseDown {
            pos,
            button,
            modifiers: mods,
        };
        if let Some(path) = modal_path {
            // Hit-test inside the modal subtree so its children get the
            // click, with the same click-to-focus rule as the normal path
            // (text fields in dialogs need focus to type).
            let path = self.extend_modal_path(&path, pos);
            if widget_at_path(&mut self.root, &path).is_focusable() {
                self.set_focus(Some(path.clone()));
            } else {
                self.set_focus(None);
            }
            if dispatch_event(&mut self.root, &path, &event, pos).is_consumed() {
                self.captured = Some(path);
            }
            return;
        }
        let hit = self.compute_hit(pos);

        // Click-to-focus: if the hit widget is focusable, give it focus.
        if let Some(ref path) = hit {
            let w = widget_at_path(&mut self.root, path);
            if w.is_focusable() {
                self.set_focus(Some(path.clone()));
            } else {
                self.set_focus(None);
            }
        } else {
            self.set_focus(None);
        }

        if let Some(mut path) = hit {
            let result = dispatch_event(&mut self.root, &path, &event, pos);
            if result.is_consumed() {
                self.maybe_bring_to_front(&mut path);
                let capture_path = self.compute_hit(pos).unwrap_or(path);
                self.captured = Some(capture_path);
            }
        }
        // NO blanket request_draw.  Mouse-down on an inert area must not
        // cause a repaint.  Each widget that changes visual state in
        // response to a MouseDown (button press, window raise, focus
        // indicator on the focus-gained widget, etc.) is responsible for
        // calling `crate::animation::request_draw` itself.
    }

    /// Mouse button released. `screen_y` is Y-down.
    pub fn on_mouse_up(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        button: MouseButton,
        mods: Modifiers,
    ) {
        let screen = self.flip_y(screen_x, screen_y);
        // On-screen keyboard owns release events on its panel; releases
        // here commit a key tap and synthesize a `KeyDown`. After
        // consumption we drain the synthetic-key queue so the focused
        // text widget receives the character in the same frame.
        if crate::widgets::on_screen_keyboard::handle_software_keyboard_mouse_up(
            screen, button, mods,
        ) {
            self.captured = None;
            self.drain_keyboard_synthetic_keys();
            return;
        }
        let pos = super::keyboard_scroll::lift_to_world(screen);
        set_current_mouse_world(pos);
        let event = Event::MouseUp {
            pos,
            button,
            modifiers: mods,
        };
        if let Some(path) = active_modal_path(self.root.as_ref()) {
            // Deliver the release where the press was captured (button
            // click completion), falling back to the hit inside the modal.
            let path = self
                .captured
                .take()
                .unwrap_or_else(|| self.extend_modal_path(&path, pos));
            dispatch_event(&mut self.root, &path, &event, pos);
            return;
        }
        // Deliver release to captured widget first (if any), then clear capture.
        if let Some(path) = self.captured.take() {
            dispatch_event(&mut self.root, &path, &event, pos);
        } else {
            let hit = self.compute_hit(pos);
            if let Some(path) = hit {
                dispatch_event(&mut self.root, &path, &event, pos);
            }
        }
    }
}
