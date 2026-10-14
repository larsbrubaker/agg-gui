//! The runner's file drags and drops: files dragged in from outside the
//! window (a file manager, a browser's file picker) hovering over a widget,
//! leaving, and dropping onto it, by path (native shells) or by name and
//! contents (the browser shell).
//!
//! Rust-only gestures. C#'s `AutomationRunner` has no file-drop method (its
//! tests set `DragFiles` on a `MouseEventArgs` and call the widget's handler);
//! here a drop reaches the `App` the way the shells deliver one, as a
//! [`ForwarderEvent`] through the window's driver
//! ([`UiDriver::send`]): `FileDragHover` → `App::on_file_drag_hover`,
//! `FileDragLeave` → `App::on_file_drag_leave`, `FileDropped` →
//! `App::on_file_dropped`, `FileDataDropped` → `App::on_file_data_dropped`.
//!
//! Each gesture onto a named widget is shaped like
//! [`AutomationRunner::drop_by_name`] (`drag.rs`): find the widget, move the
//! pointer onto it in eased, paced steps (`pointer.rs`), draw, deliver the
//! event at the pointer, draw. The file events carry their position
//! explicitly (physical pixels, Y-down, the pointer space), as the shells
//! send them. The pointer's own moves go through the run's
//! [`InputMethod`](crate::input::InputMethod); the file events do not (C#'s
//! `IInputMethod` has no file drop), they go straight to the driver.

use std::path::PathBuf;

use agg_gui::shell_input::ForwarderEvent;
use agg_gui::DroppedFileData;

use super::{AutomationRunner, DragOpts};
use crate::driver::UiDriver;
use crate::input::Point2D;

impl AutomationRunner {
    /// Move onto the widget named `widget_name` (as
    /// [`drop_by_name`](Self::drop_by_name) does), draw, and deliver `event`
    /// built at that pointer position, then draw. `opts.button` is not used:
    /// a file drag holds no button of this window's. Panics with C#'s
    /// not-found message (naming `operation`) when no such widget turns up.
    fn file_gesture_by_name(
        &mut self,
        operation: &str,
        widget_name: &str,
        opts: &DragOpts,
        event: impl FnOnce(f64, f64) -> ForwarderEvent,
    ) -> &mut Self {
        let (widget, offset_hint) = self.drag_target(operation, widget_name, opts);
        let screen_position =
            self.drag_point(operation, &widget, opts.offset, offset_hint, opts.origin);
        self.set_mouse_cursor_position(screen_position.x, screen_position.y);
        self.wait_for_draw();
        self.send_file_event(screen_position, event);
        self.wait_for_draw();
        self
    }

    fn send_file_event(&mut self, at: Point2D, event: impl FnOnce(f64, f64) -> ForwarderEvent) {
        self.check_not_timed_out();
        let event = event(f64::from(at.x), f64::from(at.y));
        UiDriver::send(&mut self.driver, event);
    }

    /// Drop `paths` on the widget named `widget_name`, as a native shell
    /// delivers a file drop (`App::on_file_dropped`): a stepped move onto
    /// the widget (`opts.offset` from `opts.origin`), a draw, the drop at
    /// the pointer, a draw. A drag hover in progress
    /// ([`file_drag_hover_by_name`](Self::file_drag_hover_by_name)) ends
    /// with the drop, as it does for a real one. Panics when no such widget
    /// turns up.
    pub fn drop_files_by_name(
        &mut self,
        widget_name: &str,
        opts: &DragOpts,
        paths: &[PathBuf],
    ) -> &mut Self {
        let paths = paths.to_vec();
        self.file_gesture_by_name("DropFilesByName", widget_name, opts, |x, y| {
            ForwarderEvent::FileDropped { x, y, paths }
        })
    }

    /// Drop `files` (each a name and its contents) on the widget named
    /// `widget_name`, as the browser shell delivers a file drop
    /// (`App::on_file_data_dropped`); otherwise as
    /// [`drop_files_by_name`](Self::drop_files_by_name).
    pub fn drop_file_data_by_name(
        &mut self,
        widget_name: &str,
        opts: &DragOpts,
        files: Vec<DroppedFileData>,
    ) -> &mut Self {
        self.file_gesture_by_name("DropFileDataByName", widget_name, opts, |x, y| {
            ForwarderEvent::FileDataDropped { x, y, files }
        })
    }

    /// A file drag naming `paths` (none, as the browser reveals none until
    /// the drop) moves over the widget named `widget_name`
    /// (`App::on_file_drag_hover`): a stepped move onto the widget, a draw,
    /// the hover at the pointer, a draw. End it with
    /// [`file_drag_leave`](Self::file_drag_leave) or a drop. Panics when no
    /// such widget turns up.
    pub fn file_drag_hover_by_name(
        &mut self,
        widget_name: &str,
        opts: &DragOpts,
        paths: &[PathBuf],
    ) -> &mut Self {
        let paths = paths.to_vec();
        self.file_gesture_by_name("FileDragHoverByName", widget_name, opts, |x, y| {
            ForwarderEvent::FileDragHover { x, y, paths }
        })
    }

    /// The file drag leaves the window or is cancelled
    /// (`App::on_file_drag_leave`), then a draw. With no drag in progress
    /// the app ignores it.
    pub fn file_drag_leave(&mut self) -> &mut Self {
        self.check_not_timed_out();
        UiDriver::send(&mut self.driver, ForwarderEvent::FileDragLeave);
        self.wait_for_draw()
    }
}
