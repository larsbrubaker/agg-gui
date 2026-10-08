//! The runner's pointer gestures — the port of `ClickByName`,
//! `DoubleClickByName`, `ClickWidget`,
//! `RightClickByName`, `RightClickWidget`, `MoveToByName`,
//! `SetMouseCursorPosition`, `CurrentMousePosition`, `MoveMouseToWidget`,
//! `PaceMouseMove`, `HoldButton` and the point forms of
//! `SystemWindowToScreen` / `ScreenToSystemWindow` (agg-sharp
//! `GuiAutomation/AutomationRunner.cs`).
//!
//! The pointer is real input: every move and button goes through the run's
//! [`InputMethod`] (by default [`SimulatedInput`], the shells' input
//! forwarder). A move is never a jump: it is [`AutomationConfig::mouse_move_steps`]
//! intermediate positions eased with Cubic.Out, then the target, because
//! hover, drag tracking and tooltips key off seeing the pointer travel. Each
//! step waits for the UI to take it (`PaceMouseMove`: one pumped frame while
//! the move's [`AutomationConfig::time_to_move_mouse`] has time left), and a
//! held button waits one frame before its release (`HoldButton`). A click
//! ends as C#'s does: draw, draw, `delay(0.2)`.
//!
//! Widget offsets ([`ClickOpts::offset`]) are logical units, Y-up from the
//! widget's lower-left corner, as in C#; the pointer itself is physical
//! pixels, Y-down ([`input`](crate::input) module docs). The name lookups
//! these gestures start from are `named.rs`'s.
//!
//! [`AutomationConfig::mouse_move_steps`]: super::AutomationConfig::mouse_move_steps
//! [`AutomationConfig::time_to_move_mouse`]: super::AutomationConfig::time_to_move_mouse

use std::time::Duration;

use agg_gui::clock;

use super::{AutomationRunner, WaitOpts, DEFAULT_WIDGET_WAIT_SECONDS};
use crate::input::{InputMethod, MouseAction, Point2D};
use crate::search_region::SearchRegion;
use crate::tree_query::{self, WidgetHandle};

/// C# `ClickOrigin`: whether a click's offset is measured from the
/// widget's lower-left corner or from its center (the lookup's offset hint).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClickOrigin {
    LowerLeft,
    #[default]
    Center,
}

/// How a named gesture finds its widget and where on it the pointer goes:
/// C#'s optional `searchRegion`, `offset`, `origin`, `isDoubleClick` and
/// `secondsToWait` parameters, with C#'s defaults.
#[derive(Clone, Copy)]
pub struct ClickOpts<'a> {
    /// Only widgets whose screen rectangle overlaps this region.
    pub search_region: Option<&'a SearchRegion>,
    /// Logical units, Y-up, from the widget's lower-left corner — or, with
    /// [`ClickOrigin::Center`], from its center.
    pub offset: Point2D,
    pub origin: ClickOrigin,
    /// Two full press/release pairs, the second press reporting 2 clicks.
    /// Ignored by [`AutomationRunner::move_to_by_name`].
    pub is_double_click: bool,
    /// How long to wait for the name to show up.
    pub secs_to_wait: f64,
}

impl Default for ClickOpts<'_> {
    fn default() -> Self {
        Self {
            search_region: None,
            offset: Point2D::ZERO,
            origin: ClickOrigin::Center,
            is_double_click: false,
            secs_to_wait: DEFAULT_WIDGET_WAIT_SECONDS,
        }
    }
}

impl<'a> ClickOpts<'a> {
    fn wait_opts(&self) -> WaitOpts<'a> {
        WaitOpts {
            secs_to_wait: self.secs_to_wait,
            search_region: self.search_region,
            only_visible: true,
        }
    }
}

/// C# `Easing.Cubic.Out`, in C#'s operation order.
pub fn cubic_out(k: f64) -> f64 {
    let k = k - 1.0;
    1.0 + (k * k * k)
}

/// The positions one stepped move from `start` to `end` passes through, in
/// order: `steps` eased intermediate points (`start + delta * Cubic.Out(i /
/// steps)`, truncated to whole pixels as C#'s `(int)` cast does), then
/// `end` itself.
pub fn mouse_move_steps(start: Point2D, end: Point2D, steps: u32) -> Vec<Point2D> {
    (0..steps)
        .map(|i| eased_step(start, end, i, steps))
        .chain(std::iter::once(end))
        .collect()
}

fn eased_step(start: Point2D, end: Point2D, i: u32, steps: u32) -> Point2D {
    let (start_x, start_y) = (f64::from(start.x), f64::from(start.y));
    let delta_x = f64::from(end.x) - start_x;
    let delta_y = f64::from(end.y) - start_y;
    let ratio = cubic_out(f64::from(i) / f64::from(steps));
    // Truncation, as C#'s `(int)`; a window's pixels never saturate.
    Point2D::new(
        (start_x + delta_x * ratio) as i32,
        (start_y + delta_y * ratio) as i32,
    )
}

impl AutomationRunner {
    /// C# `CurrentMousePosition`: the pointer, in physical pixels, Y-down.
    pub fn current_mouse_position(&self) -> Point2D {
        self.check_not_timed_out();
        self.input.current_mouse_position()
    }

    /// The run's input method (C# `InputMethod`).
    pub fn input_method(&self) -> &dyn InputMethod {
        self.check_not_timed_out();
        self.input.as_ref()
    }

    /// Replace the run's input method (C# `OverrideInputSystem`).
    pub fn set_input_method(&mut self, input: Box<dyn InputMethod>) {
        self.check_not_timed_out();
        self.input = input;
    }

    /// C# `SystemWindowToScreen` for a point: a window point (logical, Y-up)
    /// as a pointer position (physical pixels, Y-down), truncating to whole
    /// pixels as [`window_rect_to_screen`](Self::window_rect_to_screen) does.
    pub fn window_to_pointer(&self, point_on_window: Point2D) -> Point2D {
        let scale = agg_gui::ux_scale::effective_scale();
        let (_, height_px) = self.driver.size_px();
        Point2D::new(
            (f64::from(point_on_window.x) * scale) as i32,
            height_px as i32 - (f64::from(point_on_window.y) * scale) as i32,
        )
    }

    /// C# `ScreenToSystemWindow` for a point: a pointer position as a window
    /// point (logical, Y-up), rounded as C#'s `Point2D` is.
    pub fn pointer_to_window(&self, point_on_screen: Point2D) -> Point2D {
        let scale = agg_gui::ux_scale::effective_scale().max(1e-6);
        let (_, height_px) = self.driver.size_px();
        Point2D::from_f64(
            f64::from(point_on_screen.x) / scale,
            (f64::from(height_px) - f64::from(point_on_screen.y)) / scale,
        )
    }

    /// C# `PaceMouseMove`: wait for the UI to take the step just sent, for
    /// no longer than the move's remaining share of `time_to_move_mouse`.
    /// `move_elapsed` is the UI time since the move's first step, so the cap
    /// covers the move as a whole.
    fn pace_mouse_move(&mut self, move_elapsed: Duration) {
        let remaining_milliseconds =
            (self.config.time_to_move_mouse * 1000.0 - move_elapsed.as_secs_f64() * 1000.0) as i32;
        self.wait_for_pending_ui_work(remaining_milliseconds);
    }

    /// C# `HoldButton`: hold a pressed button until the UI has taken the
    /// press, for no longer than `up_delay`.
    fn hold_button(&mut self) {
        self.wait_for_pending_ui_work((self.config.up_delay * 1000.0) as i32);
    }

    fn move_pointer(&mut self, x: i32, y: i32) {
        self.input.set_cursor_position(&mut self.driver, x, y);
    }

    fn send_mouse(&mut self, action: MouseAction, at: Point2D, clicks: u32) {
        self.input
            .mouse_event(&mut self.driver, action, at.x, at.y, clicks);
    }

    /// C# `SetMouseCursorPosition(x, y)`: move the pointer to (`x`, `y`)
    /// (physical pixels, Y-down) in eased, paced steps.
    pub fn set_mouse_cursor_position(&mut self, x: i32, y: i32) {
        self.check_not_timed_out();
        let start = self.input.current_mouse_position();
        let end = Point2D::new(x, y);
        let steps = self.config.mouse_move_steps;

        let move_started = clock::now();
        for i in 0..steps {
            let current = eased_step(start, end, i, steps);
            self.move_pointer(current.x, current.y);
            self.pace_mouse_move(clock::since(move_started));
        }

        self.move_pointer(end.x, end.y);
    }

    /// C# `SetMouseCursorPosition(systemWindow, x, y)`: move the pointer to
    /// the window point (`x`, `y`) (logical, Y-up).
    pub fn set_mouse_cursor_position_in_window(&mut self, x: i32, y: i32) {
        let screen_position = self.window_to_pointer(Point2D::new(x, y));
        self.set_mouse_cursor_position(screen_position.x, screen_position.y);
    }

    /// Where the pointer goes on `widget`: its window rectangle's lower-left
    /// corner plus `offset`, as a pointer position. `None` once the widget
    /// has left the tree.
    fn widget_pointer_position(&self, widget: &WidgetHandle, offset: Point2D) -> Option<Point2D> {
        let child_bounds = tree_query::screen_rect(self.driver.root(), widget)?;
        let on_window = Point2D::from_f64(
            child_bounds.left() + f64::from(offset.x),
            child_bounds.bottom() + f64::from(offset.y),
        );
        Some(self.window_to_pointer(on_window))
    }

    /// C# `MoveMouseToWidget`: a stepped move onto `widget`, re-aiming at
    /// every step so a widget that moves while the pointer travels is still
    /// hit. Returns where the pointer ended.
    fn move_mouse_to_widget(
        &mut self,
        widget: &WidgetHandle,
        mut offset: Point2D,
        offset_hint: Point2D,
        origin: ClickOrigin,
        operation: &str,
    ) -> Point2D {
        let gone = || panic!("{operation} Failed: the widget is no longer in the window");
        let start = self.input.current_mouse_position();
        if origin == ClickOrigin::Center {
            offset = offset + offset_hint;
        }
        let mut screen_position = self
            .widget_pointer_position(widget, offset)
            .unwrap_or_else(gone);

        let move_started = clock::now();
        let steps = self.config.mouse_move_steps;
        for i in 0..steps {
            screen_position = self
                .widget_pointer_position(widget, offset)
                .unwrap_or_else(gone);
            let current = eased_step(start, screen_position, i, steps);
            self.move_pointer(current.x, current.y);
            self.pace_mouse_move(clock::since(move_started));
        }

        self.move_pointer(screen_position.x, screen_position.y);
        screen_position
    }

    /// The offset hint of a widget clicked by handle: the center of its
    /// local bounds (C# `widget.LocalBounds.Center`).
    fn center_hint(&self, widget: &WidgetHandle) -> Point2D {
        let b = widget
            .widget(self.driver.root())
            .map(|w| w.bounds())
            .unwrap_or_default();
        Point2D::from_f64(b.width / 2.0, b.height / 2.0)
    }

    /// C# `ClickByName` with its defaults: click the widget named
    /// `widget_name` at its center. Panics with
    /// [`widget_not_found_message`](Self::widget_not_found_message) when no
    /// such widget turns up.
    pub fn click_by_name(&mut self, widget_name: &str) -> &mut Self {
        self.click_by_name_with(widget_name, &ClickOpts::default())
    }

    /// C# `ClickByName`: look for a visible widget named `widget_name` (as
    /// [`get_widget_hit_by_name`](Self::get_widget_hit_by_name) chooses it)
    /// and click it. Panics with
    /// [`widget_not_found_message`](Self::widget_not_found_message) when no
    /// such widget turns up.
    pub fn click_by_name_with(&mut self, widget_name: &str, opts: &ClickOpts) -> &mut Self {
        let Some(hit) = self.get_widget_hit_by_name(widget_name, &opts.wait_opts()) else {
            panic!(
                "{}",
                Self::widget_not_found_message("ClickByName", widget_name)
            );
        };
        let offset_hint = Point2D::from_f64(hit.offset_hint.x, hit.offset_hint.y);
        self.click_widget_at(
            &hit.handle,
            opts.origin,
            opts.offset,
            offset_hint,
            opts.is_double_click,
        );
        self
    }

    /// C# `DoubleClickByName` with its defaults (a 2 s wait).
    pub fn double_click_by_name(&mut self, widget_name: &str) -> &mut Self {
        self.double_click_by_name_with(
            widget_name,
            &ClickOpts {
                secs_to_wait: 2.0,
                ..ClickOpts::default()
            },
        )
    }

    /// C# `DoubleClickByName`: [`click_by_name_with`](Self::click_by_name_with)
    /// as a double click — down(1), up, down(2) back to back, then the hold
    /// and the release.
    pub fn double_click_by_name_with(&mut self, widget_name: &str, opts: &ClickOpts) -> &mut Self {
        self.click_by_name_with(
            widget_name,
            &ClickOpts {
                is_double_click: true,
                ..*opts
            },
        )
    }

    /// C# `ClickWidget`: click `widget` at its center.
    pub fn click_widget(&mut self, widget: &WidgetHandle, is_double_click: bool) -> &mut Self {
        self.check_not_timed_out();
        let offset_hint = self.center_hint(widget);
        self.click_widget_at(
            widget,
            ClickOrigin::Center,
            Point2D::ZERO,
            offset_hint,
            is_double_click,
        );
        self
    }

    /// C#'s private `ClickWidget`: move onto the widget, press, hold,
    /// release, then draw, draw and `delay(0.2)`.
    fn click_widget_at(
        &mut self,
        widget: &WidgetHandle,
        origin: ClickOrigin,
        offset: Point2D,
        offset_hint: Point2D,
        is_double_click: bool,
    ) {
        let screen_position =
            self.move_mouse_to_widget(widget, offset, offset_hint, origin, "ClickWidget");
        self.send_mouse(MouseAction::LeftDown, screen_position, 0);

        if !is_double_click {
            // Only a single click can afford to settle here; for a double
            // click this frame would be spent out of the time the two
            // presses have to share (see below).
            self.wait_for_draw();
        }

        if is_double_click {
            // A real double click is two complete press/release pairs —
            // down(1) up down(2) up — with only the second press reporting a
            // click count of 2 (ups always report 1). The count is stated on
            // the second press rather than inferred from event spacing, and
            // nothing is waited on in between: a widget that asks during its
            // own press whether it was double-clicked compares against the
            // first press, and a loaded machine could spend the whole
            // double-click window on intervening frames. So the three events
            // go back to back, and the draws happen after the pair.
            self.send_mouse(MouseAction::LeftUp, screen_position, 0);
            self.send_mouse(MouseAction::LeftDown, screen_position, 2);
        }

        self.hold_button();

        self.send_mouse(MouseAction::LeftUp, screen_position, 0);

        self.wait_for_draw();

        // One wait just isn't enough sometimes; there can be more deferred
        // processing going on.
        self.wait_for_draw();

        self.delay(0.2);
    }

    /// C# `RightClickByName` with its defaults.
    pub fn right_click_by_name(&mut self, widget_name: &str) -> &mut Self {
        self.right_click_by_name_with(widget_name, &ClickOpts::default())
    }

    /// C# `RightClickByName`: as [`click_by_name_with`](Self::click_by_name_with)
    /// with the right button. Panics with C#'s message (which names
    /// `ClickByName`) when no such widget turns up.
    pub fn right_click_by_name_with(&mut self, widget_name: &str, opts: &ClickOpts) -> &mut Self {
        let Some(hit) = self.get_widget_hit_by_name(widget_name, &opts.wait_opts()) else {
            panic!(
                "{}",
                Self::widget_not_found_message("ClickByName", widget_name)
            );
        };
        let offset_hint = Point2D::from_f64(hit.offset_hint.x, hit.offset_hint.y);
        self.right_click_widget_at(
            &hit.handle,
            opts.origin,
            opts.offset,
            offset_hint,
            opts.is_double_click,
        );
        self
    }

    /// C# `RightClickWidget`: right-click `widget` at its center.
    pub fn right_click_widget(&mut self, widget: &WidgetHandle) -> &mut Self {
        self.check_not_timed_out();
        let offset_hint = self.center_hint(widget);
        self.right_click_widget_at(
            widget,
            ClickOrigin::Center,
            Point2D::ZERO,
            offset_hint,
            false,
        );
        self
    }

    /// C#'s private `RightClickWidget`: unlike the left click, a right
    /// double click draws between its events.
    fn right_click_widget_at(
        &mut self,
        widget: &WidgetHandle,
        origin: ClickOrigin,
        offset: Point2D,
        offset_hint: Point2D,
        is_double_click: bool,
    ) {
        let screen_position =
            self.move_mouse_to_widget(widget, offset, offset_hint, origin, "RightClickWidget");
        self.send_mouse(MouseAction::RightDown, screen_position, 0);
        self.wait_for_draw();

        if is_double_click {
            // The same double-click shape as the left click: two full
            // press/release pairs, the second press reporting 2 clicks.
            self.hold_button();
            self.send_mouse(MouseAction::RightUp, screen_position, 0);
            self.wait_for_draw();

            self.send_mouse(MouseAction::RightDown, screen_position, 2);
            self.wait_for_draw();
        }

        self.hold_button();

        self.send_mouse(MouseAction::RightUp, screen_position, 0);

        self.wait_for_draw();

        self.delay(0.2);
    }

    /// C# `MoveToByName`: move the pointer onto the widget named
    /// `widget_name` (a stepped move to where a click would land). Returns
    /// false when no such widget turns up.
    pub fn move_to_by_name(&mut self, widget_name: &str, opts: &ClickOpts) -> bool {
        let Some(hit) = self.get_widget_hit_by_name(widget_name, &opts.wait_opts()) else {
            return false;
        };
        let mut offset = opts.offset;
        if opts.origin == ClickOrigin::Center {
            offset = offset + Point2D::from_f64(hit.offset_hint.x, hit.offset_hint.y);
        }
        let Some(screen_position) = self.widget_pointer_position(&hit.handle, offset) else {
            return false;
        };
        self.set_mouse_cursor_position(screen_position.x, screen_position.y);
        true
    }
}
