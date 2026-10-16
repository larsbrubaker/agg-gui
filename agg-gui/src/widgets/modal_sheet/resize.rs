//! Opt-in edge / corner resizing for [`ModalSheet`]: the grab-band hit test,
//! the drag → panel-size mapping and the resize cursors. A desktop dialog is
//! a resizable OS window with a minimum size (agg-sharp `DialogWindow`); a
//! sheet that opts in with [`ModalSheet::with_resizable`] behaves the same
//! inside the app. The grabbed edge follows the pointer while the opposite
//! edge (for a corner, the opposite two) stays where it was, and the panel
//! keeps that place afterwards (`drag.rs` holds the placement). Split out of
//! `modal_sheet.rs`, which keeps layout, paint and key handling; the shared
//! direction enum and cursors come from
//! [`Window`](crate::widgets::window::Window)'s resize code.

use crate::cursor::set_cursor_icon;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::widgets::window::{resize_cursor, ResizeDir};

use super::ModalSheet;

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
    /// Panel rect at the press, sheet-local.
    start_panel: Rect,
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
                    start_panel: self.panel,
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

    /// Resize for the pointer at `pos`: the dragged edges follow it, the
    /// others stay put, the size is held to the minimum and the dragged
    /// edges to the host bounds. Remembers the new place and reports a
    /// changed size.
    fn apply_resize_drag(&mut self, drag: ResizeDrag, pos: Point) {
        let p = drag.start_panel;
        let (host, min) = (self.host_size, self.min_panel_size);
        let dx = pos.x - drag.start.x;
        let dy = pos.y - drag.start.y;
        let (east, west) = match drag.dir {
            ResizeDir::E | ResizeDir::NE | ResizeDir::SE => (true, false),
            ResizeDir::W | ResizeDir::NW | ResizeDir::SW => (false, true),
            ResizeDir::N | ResizeDir::S => (false, false),
        };
        // Y-up: north is the panel's top edge.
        let (north, south) = match drag.dir {
            ResizeDir::N | ResizeDir::NE | ResizeDir::NW => (true, false),
            ResizeDir::S | ResizeDir::SE | ResizeDir::SW => (false, true),
            ResizeDir::E | ResizeDir::W => (false, false),
        };
        let (x, w) = resize_axis(p.x, p.width, dx, east, west, host.width, min.width);
        let (y, h) = resize_axis(p.y, p.height, dy, north, south, host.height, min.height);
        let rect = Rect::new(x, y, w, h);
        self.remember_panel_rect(rect);
        let size = Size::new(w, h);
        if size != self.panel_size {
            self.panel_size = size;
            if let Some(cb) = &self.on_size_changed {
                cb(size);
            }
        }
        crate::animation::request_draw();
    }
}

/// One axis of a resize: the panel spans `start..start + len` at the press
/// and the pointer has moved `d`. With `grow_end` the far edge follows the
/// pointer and the near one stays; with `grow_start` the reverse; with
/// neither the axis is unchanged. The moving edge is rounded to a whole unit
/// and kept inside `0..=host`, then the length is held to `min` measured
/// from the fixed edge, so the fixed edge never drifts. Returns the new
/// start and length.
fn resize_axis(
    start: f64,
    len: f64,
    d: f64,
    grow_end: bool,
    grow_start: bool,
    host: f64,
    min: f64,
) -> (f64, f64) {
    let end = start + len;
    if grow_end {
        let moved = (end + d).round().min(host);
        (start, (moved - start).max(min).max(0.0))
    } else if grow_start {
        let moved = (start + d).round().max(0.0);
        let new_len = (end - moved).max(min).max(0.0);
        (end - new_len, new_len)
    } else {
        (start, len)
    }
}
