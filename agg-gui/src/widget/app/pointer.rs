//! Pointer-input methods on [`App`] — mouse move/down/up plus the
//! modal-subtree hit extension they share. Split out of `app.rs`
//! (800-line guardrail); keyboard routing is in `keyboard.rs`, wheel
//! routing stays in `app.rs`.

use super::tree_paths::widget_at_path;
use crate::event::{Event, Modifiers, MouseButton};
use crate::geometry::Point;
use crate::widget::tree::{active_modal_path, dispatch_event, hit_test_subtree};
use crate::widget::tree_inspector::set_current_mouse_world;
use crate::widget::{App, Widget};

/// The click-to-focus rule: a pointer press focuses the hit widget when it
/// is focusable and hasn't opted out via
/// [`WidgetBase::focus_on_click`](crate::WidgetBase::focus_on_click).
/// Widgets without a `WidgetBase` always accept click focus.
fn takes_click_focus(w: &dyn Widget) -> bool {
    w.is_focusable() && w.widget_base().map_or(true, |b| b.focus_on_click)
}

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
            if let Some(t) = widget.child_transform() {
                t.inverse_transform(&mut pos.x, &mut pos.y);
            }
            pos = Point::new(pos.x - bounds.x, pos.y - bounds.y);
            widget = child.as_ref();
        }
        let mut full = modal_path.to_vec();
        if let Some(sub_path) = hit_test_subtree(widget, pos) {
            full.extend(sub_path);
        }
        full
    }

    /// Mouse cursor moved with an explicit modifier state. Equivalent to
    /// [`App::on_modifiers_changed`] followed by [`App::on_mouse_move`], for
    /// shells whose pointer events carry modifiers (browser `MouseEvent`).
    pub fn on_mouse_move_mods(&mut self, screen_x: f64, screen_y: f64, mods: Modifiers) {
        self.on_modifiers_changed(mods);
        self.on_mouse_move(screen_x, screen_y);
    }

    /// Mouse cursor moved. `screen_y` is Y-down physical pixels.
    /// `Event::MouseMove` carries no modifiers; widgets read
    /// [`crate::event::current_modifiers`] while handling it.
    pub fn on_mouse_move(&mut self, screen_x: f64, screen_y: f64) {
        // Reset cursor so the hovered widget can set it; Default if nothing sets it.
        self.resolve_tracked_paths();
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
            self.update_hover_chain(Some(&path));
            let event = Event::MouseMove { pos };
            dispatch_event(&mut self.root, &path, &event, pos);
            self.store_hovered(Some(path));
            return;
        }
        self.dispatch_mouse_move(pos);
    }

    /// Mouse button pressed. `screen_y` is Y-down physical pixels. The
    /// press reports a click count of 1 that is not stated
    /// ([`crate::event::current_click_count`]); a shell that counts clicks
    /// goes through [`InputForwarder`](crate::shell_input::InputForwarder),
    /// and a caller that knows the count uses
    /// [`on_mouse_down_clicks`](Self::on_mouse_down_clicks).
    pub fn on_mouse_down(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        button: MouseButton,
        mods: Modifiers,
    ) {
        self.on_mouse_down_counted(screen_x, screen_y, button, mods, 1, false);
    }

    /// Mouse button pressed with a stated click count (C#'s
    /// `MouseEventArgs.Clicks`): 2 for the second press of a double click,
    /// 1 for a single click. Widgets read it while handling the press
    /// ([`crate::event::current_click_count`],
    /// [`crate::event::is_double_click`]), and a stated count overrides the
    /// text editors' own multi-click timing. `screen_y` is Y-down physical
    /// pixels.
    pub fn on_mouse_down_clicks(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        button: MouseButton,
        mods: Modifiers,
        clicks: u32,
    ) {
        self.on_mouse_down_counted(screen_x, screen_y, button, mods, clicks, true);
    }

    /// A press whose click count is `clicks`; `stated` when its sender said
    /// so rather than the forwarder working it out (see
    /// `event::click_count`).
    pub(crate) fn on_mouse_down_counted(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        button: MouseButton,
        mods: Modifiers,
        clicks: u32,
        stated: bool,
    ) {
        crate::event::begin_press(clicks, stated);
        self.on_modifiers_changed(mods);
        self.resolve_tracked_paths();
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
        // A press hides any visible central tooltip and keeps it suppressed
        // until the pointer leaves and re-enters a tipped widget.
        crate::widgets::tooltip::controller::on_pointer_down();
        // Count this press so widgets running their own multi-click gesture
        // (Scene's background double-click) can detect an intervening press
        // even when a hosted child consumes it before it can bubble.
        crate::animation::bump_pointer_press_epoch();
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
            self.update_hover_chain(Some(&path));
            if takes_click_focus(widget_at_path(&mut self.root, &path)) {
                self.set_focus(Some(path.clone()));
            } else {
                self.set_focus(None);
            }
            if dispatch_event(&mut self.root, &path, &event, pos).is_consumed() {
                self.store_captured(Some(path));
            }
            return;
        }
        let hit = self.compute_hit(pos);
        // The press announces where it landed (agg-sharp's `OnMouseDown`
        // updates `UnderMouseState`), so a press with no move before it
        // still enters and leaves the widgets it lands on and leaves.
        self.update_hover_chain(hit.as_deref());

        // Click-to-focus: if the hit widget is focusable (and accepts focus
        // from a click), give it focus.
        if let Some(ref path) = hit {
            let w = widget_at_path(&mut self.root, path);
            if takes_click_focus(w) {
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
                self.store_captured(Some(capture_path));
            }
        }
        // NO blanket request_draw.  Mouse-down on an inert area must not
        // cause a repaint.  Each widget that changes visual state in
        // response to a MouseDown (button press, window raise, focus
        // indicator on the focus-gained widget, etc.) is responsible for
        // calling `crate::animation::request_draw` itself.
    }

    /// Mouse button released. `screen_y` is Y-down. The release reports a
    /// click count of 1 while the press it ends is remembered for
    /// [`crate::event::is_double_click`] until it has been dispatched.
    pub fn on_mouse_up(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        button: MouseButton,
        mods: Modifiers,
    ) {
        crate::event::begin_release();
        self.release(screen_x, screen_y, button, mods);
        crate::event::end_release();
    }

    fn release(&mut self, screen_x: f64, screen_y: f64, button: MouseButton, mods: Modifiers) {
        self.on_modifiers_changed(mods);
        self.resolve_tracked_paths();
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
        crate::widgets::tooltip::controller::on_pointer_up();
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
            self.refresh_hover_after_release(pos);
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
        self.refresh_hover_after_release(pos);
    }

    /// Re-resolve hover (and with it the cursor icon) at the release
    /// point. Capture has just ended, so the widget that owned the drag
    /// (a splitter bar, a text selection) no longer gets every move; a
    /// release away from it would otherwise leave its drag cursor
    /// latched until the next real mouse move. Mirrors `on_mouse_move`:
    /// reset to `Default`, then let the widget now under the pointer
    /// claim the cursor. The shells re-apply `current_cursor_icon()`
    /// after every press and release.
    fn refresh_hover_after_release(&mut self, pos: Point) {
        crate::cursor::reset_cursor_icon();
        if let Some(path) = active_modal_path(self.root.as_ref()) {
            let path = self.extend_modal_path(&path, pos);
            self.update_hover_chain(Some(&path));
            dispatch_event(&mut self.root, &path, &Event::MouseMove { pos }, pos);
            self.store_hovered(Some(path));
            return;
        }
        self.dispatch_mouse_move(pos);
    }
}
