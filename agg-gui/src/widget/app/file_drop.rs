//! File drag-and-drop on [`App`]: drops carrying paths (native) or bytes
//! (browser), and the drag-hover / leave pair that lets a widget show
//! drop-target feedback before the drop. Split out of `app.rs` (800-line
//! guardrail); the event shapes are documented on [`Event::FileDropped`],
//! [`Event::FileDataDropped`], [`Event::FileDragHover`] and
//! [`Event::FileDragLeave`]. Platform shells (`agg-gui-shell`,
//! `agg-gui-web-shell`) are the callers.

use crate::event::{DroppedFileData, Event};
use crate::geometry::Point;
use crate::widget::tree::{deliver_to_all, dispatch_event, dispatch_event_broadcast};
use crate::widget::App;

impl App {
    /// Native drag-and-drop landed `paths` on the window at the given
    /// screen position. Dispatches an [`Event::FileDropped`] to the
    /// widget under the cursor (same hit-test path as `on_mouse_down`),
    /// so a widget can opt in by handling the event in `on_event`.
    ///
    /// Native shells typically receive one path per `DroppedFile` event
    /// from winit; they may forward each separately, or batch a single
    /// drag gesture into one call. The widget receives `paths` as-is.
    ///
    /// An active file drag (see [`App::on_file_drag_hover`]) is ended with
    /// [`Event::FileDragLeave`] first.
    pub fn on_file_dropped(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        paths: Vec<std::path::PathBuf>,
    ) {
        if paths.is_empty() {
            return;
        }
        self.end_file_drag();
        let pos = self.drop_pos(screen_x, screen_y);
        self.route_file_event(&Event::FileDropped { pos, paths }, pos);
    }

    /// Browser drag-and-drop landed `files` (names and contents) on the
    /// window at the given screen position. Routed exactly like
    /// [`App::on_file_dropped`], as an [`Event::FileDataDropped`].
    pub fn on_file_data_dropped(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        files: Vec<DroppedFileData>,
    ) {
        if files.is_empty() {
            return;
        }
        self.end_file_drag();
        let pos = self.drop_pos(screen_x, screen_y);
        self.route_file_event(&Event::FileDataDropped { pos, files }, pos);
    }

    /// A file drag from outside the app is over the window at the given
    /// screen position (it entered, or moved). Sends
    /// [`Event::FileDragHover`] along the same route as a drop. `paths` is
    /// what the platform reveals of the dragged files — empty in the browser.
    pub fn on_file_drag_hover(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        paths: Vec<std::path::PathBuf>,
    ) {
        self.file_drag_active = true;
        let pos = self.drop_pos(screen_x, screen_y);
        self.route_file_event(&Event::FileDragHover { pos, paths }, pos);
    }

    /// The file drag left the window or was cancelled. Sends
    /// [`Event::FileDragLeave`] to every widget, once per drag: a call with
    /// no drag in progress does nothing.
    pub fn on_file_drag_leave(&mut self) {
        self.end_file_drag();
    }

    /// Whether a file drag is over the window (a hover arrived and nothing
    /// has ended it yet).
    pub fn file_drag_active(&self) -> bool {
        self.file_drag_active
    }

    fn end_file_drag(&mut self) {
        if !std::mem::take(&mut self.file_drag_active) {
            return;
        }
        self.resolve_tracked_paths();
        deliver_to_all(self.root.as_mut(), &Event::FileDragLeave);
        crate::animation::request_draw();
    }

    fn drop_pos(&self, screen_x: f64, screen_y: f64) -> Point {
        crate::widget::keyboard_scroll::lift_to_world(self.flip_y(screen_x, screen_y))
    }

    /// Hit-test and bubble `event` from the widget under `pos`; if nothing on
    /// that path consumes it, offer it to the rest of the tree.
    fn route_file_event(&mut self, event: &Event, pos: Point) {
        self.resolve_tracked_paths();
        let hit = self.compute_hit(pos);
        let consumed = match hit {
            Some(path) => dispatch_event(&mut self.root, &path, event, pos),
            // No hit target: dispatch to the root anyway so app-level
            // handlers (e.g. "open the dropped .atmr project") can run
            // even when the user drops on chrome rather than canvas.
            None => dispatch_event(&mut self.root, &[], event, pos),
        }
        .is_consumed();
        if !consumed {
            // The widget under the drop point ignored the files. Offer
            // the event to the rest of the tree before giving up — the
            // reported position is often wrong through no fault of the
            // user (winit's Windows backend discards the OLE drop point
            // and emits no CursorMoved during the drag, so shells fall
            // back to the last pre-drag cursor position). A drop must
            // find the app's file handler even when it "lands" on
            // chrome or a sibling pane.
            dispatch_event_broadcast(&mut self.root, event, pos);
        }
        crate::animation::request_draw();
    }
}
