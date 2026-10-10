//! Opt-in edge / corner resizing for [`ModalSheet`]: the grab-band hit test,
//! the drag → panel-size mapping and the resize cursors. A desktop dialog is
//! a resizable OS window with a minimum size (agg-sharp `DialogWindow`); a
//! sheet that opts in with [`ModalSheet::with_resizable`] behaves the same
//! inside the app. The panel stays centred, so an edge drag moves both
//! opposite edges and the size changes by twice the drag — the grabbed edge
//! follows the pointer. Split out of `modal_sheet.rs`, which keeps layout,
//! paint and key handling; the shared direction enum and cursors come from
//! [`Window`](crate::widgets::window::Window)'s resize code.

use crate::cursor::set_cursor_icon;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Size};
use crate::widgets::window::{resize_cursor, ResizeDir};

use super::{ModalSheet, EDGE_MARGIN};

/// How far inside the panel edge a press still grabs the edge.
const GRAB_INSIDE: f64 = 6.0;
/// How far outside the panel edge (over the scrim) a press grabs the edge.
const GRAB_OUTSIDE: f64 = 4.0;

/// An edge / corner drag in progress.
#[derive(Clone, Copy, Debug)]
pub(super) struct ResizeDrag {
    dir: ResizeDir,
    /// Pointer position at the press, sheet-local.
    start: Point,
    /// Panel size at the press.
    start_size: Size,
}

impl ModalSheet {
    /// The edge or corner under `local` (sheet-local), when resizing is on.
    pub(super) fn resize_dir_at(&self, local: Point) -> Option<ResizeDir> {
        if !self.resizable || !self.visible.get() {
            return None;
        }
        let p = self.panel;
        let (x0, x1) = (p.x, p.x + p.width);
        let (y0, y1) = (p.y, p.y + p.height);
        if local.x < x0 - GRAB_OUTSIDE
            || local.x > x1 + GRAB_OUTSIDE
            || local.y < y0 - GRAB_OUTSIDE
            || local.y > y1 + GRAB_OUTSIDE
        {
            return None;
        }
        let near = |v: f64, edge: f64, inward: f64| {
            let d = (v - edge) * inward;
            (-GRAB_OUTSIDE..=GRAB_INSIDE).contains(&d)
        };
        // Y-up: the north edge is the panel's top, `y1`.
        let n = near(local.y, y1, -1.0);
        let s = near(local.y, y0, 1.0);
        let w = near(local.x, x0, 1.0);
        let e = near(local.x, x1, -1.0);
        match (n, e, s, w) {
            (true, true, _, _) => Some(ResizeDir::NE),
            (true, _, _, true) => Some(ResizeDir::NW),
            (_, true, true, _) => Some(ResizeDir::SE),
            (_, _, true, true) => Some(ResizeDir::SW),
            (true, _, _, _) => Some(ResizeDir::N),
            (_, true, _, _) => Some(ResizeDir::E),
            (_, _, true, _) => Some(ResizeDir::S),
            (_, _, _, true) => Some(ResizeDir::W),
            _ => None,
        }
    }

    /// Whether a resize drag is in progress (the sheet holds the pointer).
    pub(super) fn is_resizing(&self) -> bool {
        self.resize_drag.is_some()
    }

    /// Pointer handling for resizing. `Some` when the event belonged to a
    /// resize (hover cursor, press on an edge, drag, release); `None` lets
    /// the sheet's ordinary handling run.
    pub(super) fn resize_event(&mut self, event: &Event) -> Option<EventResult> {
        if !self.resizable {
            return None;
        }
        match event {
            Event::MouseMove { pos } => {
                if let Some(drag) = self.resize_drag {
                    self.apply_resize_drag(drag, *pos);
                    set_cursor_icon(resize_cursor(drag.dir));
                    return Some(EventResult::Consumed);
                }
                if let Some(dir) = self.resize_dir_at(*pos) {
                    set_cursor_icon(resize_cursor(dir));
                }
                None
            }
            Event::MouseDown { pos, .. } => {
                let dir = self.resize_dir_at(*pos)?;
                self.resize_drag = Some(ResizeDrag {
                    dir,
                    start: *pos,
                    start_size: Size::new(self.panel.width, self.panel.height),
                });
                set_cursor_icon(resize_cursor(dir));
                Some(EventResult::Consumed)
            }
            Event::MouseUp { .. } | Event::MouseCaptureLost => {
                self.resize_drag.take()?;
                crate::animation::request_draw();
                Some(EventResult::Consumed)
            }
            _ => None,
        }
    }

    /// Resize for the pointer at `pos`, clamped to the minimum size and to
    /// the host bounds minus the edge margin; report a changed size.
    fn apply_resize_drag(&mut self, drag: ResizeDrag, pos: Point) {
        let dx = pos.x - drag.start.x;
        let dy = pos.y - drag.start.y;
        // Centred panel: each edge moves by the drag, the opposite one by
        // its mirror, so the size changes by twice the drag.
        let (sx, sy) = match drag.dir {
            ResizeDir::N => (0.0, 1.0),
            ResizeDir::S => (0.0, -1.0),
            ResizeDir::E => (1.0, 0.0),
            ResizeDir::W => (-1.0, 0.0),
            ResizeDir::NE => (1.0, 1.0),
            ResizeDir::NW => (-1.0, 1.0),
            ResizeDir::SE => (1.0, -1.0),
            ResizeDir::SW => (-1.0, -1.0),
        };
        let max_w = (self.host_size.width - 2.0 * EDGE_MARGIN).max(self.min_panel_size.width);
        let max_h = (self.host_size.height - 2.0 * EDGE_MARGIN).max(self.min_panel_size.height);
        let w = (drag.start_size.width + 2.0 * sx * dx)
            .round()
            .min(max_w)
            .max(self.min_panel_size.width)
            .max(0.0);
        let h = (drag.start_size.height + 2.0 * sy * dy)
            .round()
            .min(max_h)
            .max(self.min_panel_size.height)
            .max(0.0);
        let size = Size::new(w, h);
        if size != self.panel_size {
            self.panel_size = size;
            if let Some(cb) = &self.on_size_changed {
                cb(size);
            }
            crate::animation::request_draw();
        }
    }
}
