//! The runner's file drags and drops: files dragged in from outside the
//! window (a file manager, a browser's file picker) hovering over a widget,
//! leaving, and dropping onto it, by path (native shells) or by name and
//! contents (the browser shell).
//!
//! Rust-only gestures. C#'s `AutomationRunner` has no file-drop method (its
//! tests set `DragFiles` on a `MouseEventArgs` and call the widget's handler);
//! here a drag reaches the `App` the way the shells deliver a real one, as
//! [`ForwarderEvent`]s through the window's driver ([`UiDriver::send`]):
//! `FileDragHover` → `App::on_file_drag_hover`, `FileDragLeave` →
//! `App::on_file_drag_leave`, `FileDropped` → `App::on_file_dropped`,
//! `FileDataDropped` → `App::on_file_data_dropped`.
//!
//! A real file drag owns the pointer. While it is over the window the shells
//! hear of it only as file-drag hovers (winit's `HoveredFile`, the browser's
//! `dragenter`/`dragover`), never as mouse moves, so a widget's mouse-hover
//! state (agg-sharp's `MouseEnterBounds`) does not follow a file drag. The
//! gestures here do the same:
//! - **Start.** The user picked the files up in another application, so the
//!   pointer left this window first: a drag that is not already over the
//!   window begins with the shells' `MouseLeave` (winit's `CursorLeft`, the
//!   browser's `pointerleave`) when the runner's pointer is inside it.
//! - **Travel.** The drag moves onto the named widget along the same eased,
//!   paced steps as a mouse move (`pointer.rs`), each step a
//!   `FileDragHover` at that position (none for a step outside the window,
//!   which the window would not hear of). The run's
//!   [`InputMethod`](crate::input::InputMethod) records each position
//!   ([`InputMethod::record_cursor_position`](crate::input::InputMethod::record_cursor_position))
//!   and sends nothing, so later gestures start from where the drag is.
//! - **Drop.** Native: the drop at the pointer; `App` ends the drag with
//!   `FileDragLeave` first. Browser: the shell's own `FileDragLeave`, then
//!   the data drop. The pointer stays at the drop point, inside the window,
//!   and no move follows until the next gesture makes one.
//! - **Leave.** [`AutomationRunner::file_drag_leave`]: the drag left the
//!   window. The shells report only `FileDragLeave` (winit's
//!   `HoveredFileCancelled`, the browser's `dragleave`), and the pointer is
//!   then outside the window: the runner puts it one pixel past the window
//!   edge nearest to where the drag was.
//!
//! File events carry their position explicitly (physical pixels, Y-down,
//! the pointer space), as the shells send them; they go straight to the
//! driver, not through the input method (C#'s `IInputMethod` has no file
//! drop).

use std::path::PathBuf;

use agg_gui::shell_input::ForwarderEvent;
use agg_gui::{clock, DroppedFileData};

use super::pointer::eased_step;
use super::{AutomationRunner, DragOpts};
use crate::driver::UiDriver;
use crate::input::{window_contains, Point2D};

/// Where the pointer is once a drag at `p` leaves a window of `size_px`:
/// one pixel past the nearest edge (left, right, top, bottom on a tie), so
/// it is outside even by the edges-included containment test.
fn outside_nearest_edge(p: Point2D, size_px: (u32, u32)) -> Point2D {
    let (width, height) = (i64::from(size_px.0), i64::from(size_px.1));
    let (x, y) = (i64::from(p.x), i64::from(p.y));
    let distances = [x, width - x, y, height - y];
    let nearest = (0..distances.len())
        .min_by_key(|&i| distances[i])
        .unwrap_or(0);
    // Window sizes are far below i32::MAX; the clamp only matters for a
    // window no display has.
    let clamp = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    match nearest {
        0 => Point2D::new(-1, p.y),
        1 => Point2D::new(clamp(width + 1), p.y),
        2 => Point2D::new(p.x, -1),
        _ => Point2D::new(p.x, clamp(height + 1)),
    }
}

impl<D: UiDriver> AutomationRunner<D> {
    /// Carry a file drag naming `paths` onto the widget named `widget_name`
    /// (the point [`drop_by_name`](Self::drop_by_name) would move to), as
    /// the module docs describe, then draw. Returns where the drag ended.
    /// `opts.button` is not used: a file drag holds no button of this
    /// window's. Panics with C#'s not-found message (naming `operation`)
    /// when no such widget turns up.
    fn file_drag_onto(
        &mut self,
        operation: &str,
        widget_name: &str,
        opts: &DragOpts,
        paths: &[PathBuf],
    ) -> Point2D {
        let (widget, offset_hint) = self.drag_target(operation, widget_name, opts);
        let end = self.drag_point(operation, &widget, opts.offset, offset_hint, opts.origin);
        self.check_not_timed_out();

        let start = self.input.current_mouse_position();
        if !self.driver.app().file_drag_active() && window_contains(self.driver.size_px(), start) {
            UiDriver::send(&mut self.driver, ForwarderEvent::MouseLeave);
        }

        let move_started = clock::now();
        let steps = self.config.mouse_move_steps;
        for i in 0..steps {
            let current = eased_step(start, end, i, steps);
            self.file_drag_step(current, paths);
            self.pace_mouse_move(clock::since(move_started));
        }
        self.file_drag_step(end, paths);

        self.wait_for_draw();
        end
    }

    /// One step of a file drag: the pointer is at `at`, and the window hears
    /// a hover there when `at` is inside it.
    fn file_drag_step(&mut self, at: Point2D, paths: &[PathBuf]) {
        self.input.record_cursor_position(at.x, at.y);
        if window_contains(self.driver.size_px(), at) {
            UiDriver::send(
                &mut self.driver,
                ForwarderEvent::FileDragHover {
                    x: f64::from(at.x),
                    y: f64::from(at.y),
                    paths: paths.to_vec(),
                },
            );
        }
    }

    /// Drop `paths` on the widget named `widget_name`, as a native shell
    /// delivers a file drop: the drag travels onto the widget (`opts.offset`
    /// from `opts.origin`) as hovers naming `paths`, a draw, the drop at the
    /// pointer (`App::on_file_dropped`, which ends the drag first), a draw.
    /// Panics when no such widget turns up.
    pub fn drop_files_by_name(
        &mut self,
        widget_name: &str,
        opts: &DragOpts,
        paths: &[PathBuf],
    ) -> &mut Self {
        let at = self.file_drag_onto("DropFilesByName", widget_name, opts, paths);
        UiDriver::send(
            &mut self.driver,
            ForwarderEvent::FileDropped {
                x: f64::from(at.x),
                y: f64::from(at.y),
                paths: paths.to_vec(),
            },
        );
        self.wait_for_draw()
    }

    /// Drop `files` (each a name and its contents) on the widget named
    /// `widget_name`, as the browser shell delivers a file drop: the drag
    /// travels onto the widget as hovers naming no paths (a page sees none),
    /// a draw, the shell's `FileDragLeave`, the drop at the pointer
    /// (`App::on_file_data_dropped`), a draw. Otherwise as
    /// [`drop_files_by_name`](Self::drop_files_by_name).
    pub fn drop_file_data_by_name(
        &mut self,
        widget_name: &str,
        opts: &DragOpts,
        files: Vec<DroppedFileData>,
    ) -> &mut Self {
        let at = self.file_drag_onto("DropFileDataByName", widget_name, opts, &[]);
        UiDriver::send(&mut self.driver, ForwarderEvent::FileDragLeave);
        UiDriver::send(
            &mut self.driver,
            ForwarderEvent::FileDataDropped {
                x: f64::from(at.x),
                y: f64::from(at.y),
                files,
            },
        );
        self.wait_for_draw()
    }

    /// A file drag naming `paths` (none, as the browser reveals none until
    /// the drop) travels over the widget named `widget_name` and stays
    /// there: hovers along the way (`App::on_file_drag_hover`), the last at
    /// the widget, then two draws. End it with
    /// [`file_drag_leave`](Self::file_drag_leave) or a drop. Panics when no
    /// such widget turns up.
    pub fn file_drag_hover_by_name(
        &mut self,
        widget_name: &str,
        opts: &DragOpts,
        paths: &[PathBuf],
    ) -> &mut Self {
        self.file_drag_onto("FileDragHoverByName", widget_name, opts, paths);
        self.wait_for_draw()
    }

    /// The file drag leaves the window (`App::on_file_drag_leave`), then a
    /// draw. The pointer is then outside the window, one pixel past the edge
    /// nearest to the drag, and the window was told of no mouse move: no
    /// widget is hovered. With no drag in progress the app ignores the
    /// leave and the pointer stays where it is.
    pub fn file_drag_leave(&mut self) -> &mut Self {
        self.check_not_timed_out();
        let was_dragging = self.driver.app().file_drag_active();
        UiDriver::send(&mut self.driver, ForwarderEvent::FileDragLeave);
        if was_dragging {
            let at = self.input.current_mouse_position();
            let outside = outside_nearest_edge(at, self.driver.size_px());
            self.input.record_cursor_position(outside.x, outside.y);
        }
        self.wait_for_draw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_only_outside_nearest_edge_steps_one_pixel_past_the_closest_edge() {
        let size = (300, 200);
        assert_eq!(
            outside_nearest_edge(Point2D::new(10, 100), size),
            Point2D::new(-1, 100)
        );
        assert_eq!(
            outside_nearest_edge(Point2D::new(290, 100), size),
            Point2D::new(301, 100)
        );
        assert_eq!(
            outside_nearest_edge(Point2D::new(150, 5), size),
            Point2D::new(150, -1)
        );
        assert_eq!(
            outside_nearest_edge(Point2D::new(150, 195), size),
            Point2D::new(150, 201)
        );
        for p in [Point2D::new(0, 0), Point2D::new(300, 200)] {
            assert!(!window_contains(size, outside_nearest_edge(p, size)));
        }
    }
}
