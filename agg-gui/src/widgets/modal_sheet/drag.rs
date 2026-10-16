//! Where a [`ModalSheet`]'s panel sits, and moving it: the layout placement
//! (centred until the user places it, then kept at its offset from the
//! host's top-left and inside the host) and the opt-in move drag by the
//! panel's top strip ([`ModalSheet::with_drag_strip`]), the sheet's title
//! bar. As with a [`Window`](crate::widgets::window::Window) title drag the
//! panel follows the pointer and the cursor shows a closed hand while it
//! moves. Edge resizing lives in `resize.rs` and records the panel's place
//! through [`ModalSheet::remember_panel_rect`]; layout, paint and keys stay
//! in `modal_sheet.rs`.

use crate::cursor::{set_cursor_icon, CursorIcon};
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};

use super::ModalSheet;

/// A move drag in progress.
#[derive(Clone, Copy, Debug)]
pub(super) struct MoveDrag {
    /// Pointer position at the press, sheet-local.
    start: Point,
    /// Panel rect at the press, sheet-local.
    start_panel: Rect,
}

impl ModalSheet {
    /// Let the user move the panel by pressing in its top `height` units and
    /// dragging, as a dialog moves by its title bar (default 0: the panel
    /// does not move). Only a press the content leaves unconsumed starts a
    /// move, so a close button or other control in the strip keeps working.
    pub fn with_drag_strip(mut self, height: f64) -> Self {
        self.set_drag_strip(height);
        self
    }

    /// Object-safe counterpart of [`with_drag_strip`](Self::with_drag_strip).
    pub fn set_drag_strip(&mut self, height: f64) {
        self.drag_strip = height.max(0.0);
        if self.drag_strip == 0.0 {
            self.move_drag = None;
        }
    }

    /// Height of the top strip that moves the panel (0 when moving is off).
    pub fn drag_strip(&self) -> f64 {
        self.drag_strip
    }

    /// Where the user put the panel: its top-left corner's offset from the
    /// host's top-left corner, x rightward and y downward. `None` while the
    /// panel is centred (it has not been moved or resized).
    pub fn panel_position(&self) -> Option<Point> {
        self.position
    }

    /// Place the panel's top-left corner at `position` (offset from the
    /// host's top-left, x rightward, y downward), or centre it again with
    /// `None` — a host restoring a remembered place, or re-centring a sheet
    /// it shows again. Layout keeps the panel inside the host.
    pub fn set_panel_position(&mut self, position: Option<Point>) {
        self.position = position;
        crate::animation::request_draw();
    }

    /// The panel rect for a `size` panel in a host of `host` size: centred
    /// until the user places it, then at its remembered offset, kept inside
    /// the host. On an axis where the panel is larger than the host it is
    /// centred (overhanging both sides equally) either way.
    pub(super) fn place_panel(&self, host: Size, size: Size) -> Rect {
        let (left, top_gap) = match self.position {
            Some(p) => (Some(p.x), Some(p.y)),
            None => (None, None),
        };
        // Y-up: the remembered gap is measured down from the host's top.
        let bottom = top_gap.map(|gap| host.height - gap - size.height);
        Rect::new(
            place_axis(host.width, size.width, left),
            place_axis(host.height, size.height, bottom),
            size.width,
            size.height,
        )
    }

    /// Remember `rect` (sheet-local) as where the user put the panel.
    pub(super) fn remember_panel_rect(&mut self, rect: Rect) {
        self.position = Some(Point::new(
            rect.x,
            self.host_size.height - (rect.y + rect.height),
        ));
    }

    /// Whether a move drag is in progress (the sheet holds the pointer).
    pub(super) fn is_moving(&self) -> bool {
        self.move_drag.is_some()
    }

    /// Whether `local` (sheet-local) is in the panel's top drag strip.
    fn in_drag_strip(&self, local: Point) -> bool {
        let p = self.panel;
        let top = p.y + p.height;
        self.drag_strip > 0.0
            && local.x >= p.x
            && local.x <= p.x + p.width
            && local.y <= top
            && local.y >= top - self.drag_strip.min(p.height)
    }

    /// Pointer handling for moving. `Some` when the event belonged to a
    /// move (a press in the strip, the drag, the release); `None` lets the
    /// sheet's ordinary handling run.
    pub(super) fn move_event(&mut self, event: &Event) -> Option<EventResult> {
        match event {
            Event::MouseMove { pos } => {
                let drag = self.move_drag?;
                self.apply_move_drag(drag, *pos);
                set_cursor_icon(CursorIcon::Grabbing);
                Some(EventResult::Consumed)
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } if self.in_drag_strip(*pos) => {
                self.move_drag = Some(MoveDrag {
                    start: *pos,
                    start_panel: self.panel,
                });
                set_cursor_icon(CursorIcon::Grabbing);
                crate::animation::request_draw();
                Some(EventResult::Consumed)
            }
            Event::MouseUp { .. } | Event::MouseCaptureLost => {
                self.move_drag.take()?;
                set_cursor_icon(CursorIcon::Default);
                crate::animation::request_draw();
                Some(EventResult::Consumed)
            }
            _ => None,
        }
    }

    /// Move the panel so the point grabbed stays under the pointer at
    /// `pos`, kept inside the host, and remember the place.
    fn apply_move_drag(&mut self, drag: MoveDrag, pos: Point) {
        let s = drag.start_panel;
        let x = (s.x + pos.x - drag.start.x).round();
        let y = (s.y + pos.y - drag.start.y).round();
        let host = self.host_size;
        let rect = Rect::new(
            place_axis(host.width, s.width, Some(x)),
            place_axis(host.height, s.height, Some(y)),
            s.width,
            s.height,
        );
        if rect == self.panel {
            return;
        }
        self.remember_panel_rect(rect);
        // Move the content with the panel now, so a hit test before the
        // next layout finds it where it is drawn.
        self.panel = rect;
        if let Some(child) = self.children.first_mut() {
            child.set_bounds(rect);
        }
        crate::animation::request_draw();
    }
}

/// The panel's start on one axis: `wanted` kept inside `0..=host - len`,
/// or centred when there is no wanted place or the panel is longer than the
/// host. Snapped to whole units for crisp chrome.
fn place_axis(host: f64, len: f64, wanted: Option<f64>) -> f64 {
    match wanted {
        Some(v) if len <= host => v.round().clamp(0.0, (host - len).floor()),
        _ => ((host - len) / 2.0).round(),
    }
}
