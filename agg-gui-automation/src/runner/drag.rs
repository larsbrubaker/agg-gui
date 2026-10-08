//! The runner's drags and drops — the port of `DragByName`, `DropByName`,
//! `DragDropByName`, `DragWidget`, `DragToPosition`, `Drop` and the private
//! `DragStart` (agg-sharp `GuiAutomation/AutomationRunner.cs`).
//!
//! A drag is the pointer moves of `pointer.rs` with the press and the
//! release split apart: [`AutomationRunner::drag_by_name`] moves onto a
//! widget (stepped and paced) and presses there, and
//! [`AutomationRunner::drop_by_name`] moves onto another and releases, each
//! with a draw before and after its button event, as C# does.
//! [`AutomationRunner::drop`] releases wherever the pointer is.
//!
//! Offsets are C#'s: logical units, Y-up from the widget's lower-left corner
//! (or from its center with [`ClickOrigin::Center`]). `drag_widget`'s
//! `travel` is added to the pointer position, so it is physical pixels,
//! Y-down, exactly as C# adds it in screen space.

use agg_gui::MouseButton;

use super::{AutomationRunner, ClickOrigin, WaitOpts, DEFAULT_WIDGET_WAIT_SECONDS};
use crate::input::{MouseAction, Point2D};
use crate::search_region::SearchRegion;
use crate::tree_query::WidgetHandle;

/// C#'s optional parameters of `DragByName` / `DropByName`, with C#'s
/// defaults.
#[derive(Clone, Copy)]
pub struct DragOpts<'a> {
    /// How long to wait for the name to show up.
    pub secs_to_wait: f64,
    /// Only widgets whose screen rectangle overlaps this region.
    pub search_region: Option<&'a SearchRegion>,
    /// Logical units, Y-up, from the widget's lower-left corner — or, with
    /// [`ClickOrigin::Center`], from its center.
    pub offset: Point2D,
    pub origin: ClickOrigin,
    /// The button held for the drag.
    pub button: MouseButton,
}

impl Default for DragOpts<'_> {
    fn default() -> Self {
        Self {
            secs_to_wait: DEFAULT_WIDGET_WAIT_SECONDS,
            search_region: None,
            offset: Point2D::ZERO,
            origin: ClickOrigin::Center,
            button: MouseButton::Left,
        }
    }
}

/// C#'s optional parameters of `DragDropByName`: one wait, region and button
/// for both ends, and an offset and origin for each.
#[derive(Clone, Copy)]
pub struct DragDropOpts<'a> {
    pub secs_to_wait: f64,
    pub search_region: Option<&'a SearchRegion>,
    pub offset_drag: Point2D,
    pub origin_drag: ClickOrigin,
    pub offset_drop: Point2D,
    pub origin_drop: ClickOrigin,
    pub button: MouseButton,
}

impl Default for DragDropOpts<'_> {
    fn default() -> Self {
        Self {
            secs_to_wait: DEFAULT_WIDGET_WAIT_SECONDS,
            search_region: None,
            offset_drag: Point2D::ZERO,
            origin_drag: ClickOrigin::Center,
            offset_drop: Point2D::ZERO,
            origin_drop: ClickOrigin::Center,
            button: MouseButton::Left,
        }
    }
}

impl AutomationRunner {
    /// C# `DragDropByName`: [`drag_by_name`](Self::drag_by_name) on
    /// `widget_name_drag`, then [`drop_by_name`](Self::drop_by_name) on
    /// `widget_name_drop`.
    pub fn drag_drop_by_name(
        &mut self,
        widget_name_drag: &str,
        widget_name_drop: &str,
        opts: &DragDropOpts,
    ) -> &mut Self {
        self.drag_by_name(
            widget_name_drag,
            &DragOpts {
                secs_to_wait: opts.secs_to_wait,
                search_region: opts.search_region,
                offset: opts.offset_drag,
                origin: opts.origin_drag,
                button: opts.button,
            },
        );
        self.drop_by_name(
            widget_name_drop,
            &DragOpts {
                secs_to_wait: opts.secs_to_wait,
                search_region: opts.search_region,
                offset: opts.offset_drop,
                origin: opts.origin_drop,
                button: opts.button,
            },
        )
    }

    /// The widget named `widget_name` and its offset hint, or C#'s
    /// not-found message for `operation`.
    fn drag_target(
        &mut self,
        operation: &str,
        widget_name: &str,
        opts: &DragOpts,
    ) -> (WidgetHandle, Point2D) {
        let wait = WaitOpts {
            secs_to_wait: opts.secs_to_wait,
            search_region: opts.search_region,
            only_visible: true,
        };
        let Some(hit) = self.get_widget_hit_by_name(widget_name, &wait) else {
            panic!("{}", Self::widget_not_found_message(operation, widget_name));
        };
        let hint = Point2D::from_f64(hit.offset_hint.x, hit.offset_hint.y);
        (hit.handle, hint)
    }

    /// Where the pointer goes on `widget` for `offset` from `origin`, or a
    /// panic naming `operation` once the widget has left the tree.
    fn drag_point(
        &self,
        operation: &str,
        widget: &WidgetHandle,
        mut offset: Point2D,
        offset_hint: Point2D,
        origin: ClickOrigin,
    ) -> Point2D {
        if origin == ClickOrigin::Center {
            offset = offset + offset_hint;
        }
        self.widget_pointer_position(widget, offset)
            .unwrap_or_else(|| panic!("{operation} Failed: the widget is no longer in the window"))
    }

    /// C# `DragByName`: move onto the widget named `widget_name` and press
    /// `opts.button` there. Panics with C#'s not-found message when no such
    /// widget turns up.
    pub fn drag_by_name(&mut self, widget_name: &str, opts: &DragOpts) -> &mut Self {
        let (widget, offset_hint) = self.drag_target("DragByName", widget_name, opts);
        self.drag_start(&widget, opts.origin, opts.offset, offset_hint, opts.button);
        self
    }

    /// C# `DragWidget`: press on `widget`'s center and move the pointer by
    /// `travel` (physical pixels, Y-down, added in screen space as C# does).
    /// The button stays down; [`drop`](Self::drop) releases it.
    pub fn drag_widget(
        &mut self,
        widget: &WidgetHandle,
        travel: Point2D,
        button: MouseButton,
    ) -> &mut Self {
        self.check_not_timed_out();
        let center = self.center_hint(widget);
        let start = self.drag_start(widget, ClickOrigin::Center, Point2D::ZERO, center, button);
        let screen_position = Point2D::new(start.x + travel.x, start.y + travel.y);
        self.set_mouse_cursor_position(screen_position.x, screen_position.y);
        self
    }

    /// C# `DragToPosition(window, x, y)`: press the left button where the
    /// pointer is and move to the window point (`x`, `y`) (logical, Y-up).
    pub fn drag_to_position(&mut self, x: i32, y: i32) -> &mut Self {
        self.check_not_timed_out();
        let screen_position = self.input.current_mouse_position();
        self.send_mouse(MouseAction::LeftDown, screen_position, 0);
        self.set_mouse_cursor_position_in_window(x, y);
        self
    }

    /// C#'s private `DragStart`: move onto the widget, draw, press, draw.
    /// Returns where the press landed.
    fn drag_start(
        &mut self,
        widget: &WidgetHandle,
        origin: ClickOrigin,
        offset: Point2D,
        offset_hint: Point2D,
        button: MouseButton,
    ) -> Point2D {
        let screen_position = self.drag_point("DragStart", widget, offset, offset_hint, origin);
        self.set_mouse_cursor_position(screen_position.x, screen_position.y);
        self.wait_for_draw();
        self.send_mouse(MouseAction::down(button), screen_position, 0);
        self.wait_for_draw();
        screen_position
    }

    /// C# `DropByName`: move onto the widget named `widget_name`, draw,
    /// release `opts.button` there, draw. Panics with C#'s not-found message
    /// when no such widget turns up.
    pub fn drop_by_name(&mut self, widget_name: &str, opts: &DragOpts) -> &mut Self {
        let (widget, offset_hint) = self.drag_target("DropByName", widget_name, opts);
        let screen_position =
            self.drag_point("DropByName", &widget, opts.offset, offset_hint, opts.origin);
        self.set_mouse_cursor_position(screen_position.x, screen_position.y);
        self.wait_for_draw();
        self.drop(opts.button);
        self.wait_for_draw();
        self
    }

    /// C# `Drop`: release `button` where the pointer is.
    pub fn drop(&mut self, button: MouseButton) -> &mut Self {
        self.check_not_timed_out();
        let screen_position = self.input.current_mouse_position();
        self.send_mouse(MouseAction::up(button), screen_position, 0);
        self
    }
}
