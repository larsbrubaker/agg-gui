// Edge / corner resize geometry for `Window`: the minimum height a resize
// may reach and the drag-delta → bounds mapping per resize direction. Lifted
// out of `window.rs` (which keeps `resize_dir`, the hit-test side) to keep
// that file under the 800-line limit; snapping lives in `snap_glue.rs`.
// Resized bounds round by edge (`snap_edges`) so the undragged edge stays put.

use super::*;

impl Window {
    /// Effective minimum height for this resize pass.  Honours
    /// either `tight_content_fit` (lock + floor) or
    /// `floor_content_height` (floor only) so a window whose content
    /// has a natural height > MIN_H can never be dragged smaller
    /// than its content.
    pub(super) fn effective_min_h(&self) -> f64 {
        if self.tight_content_fit || self.floor_content_height {
            let content_min = self.last_content_natural_h.get() + TITLE_H;
            MIN_H.max(content_min)
        } else {
            MIN_H
        }
    }

    /// Apply a mouse-world-space delta to bounds according to the resize direction.
    pub(super) fn apply_resize(&mut self, world_pos: Point) {
        let dx = world_pos.x - self.drag_start_world.x;
        let dy = world_pos.y - self.drag_start_world.y;
        let sb = self.drag_start_bounds;
        let min_h = self.effective_min_h();

        let (mut x, mut y, mut w, mut h) = (sb.x, sb.y, sb.width, sb.height);

        if let DragMode::Resize(dir) = self.drag_mode {
            match dir {
                ResizeDir::N => {
                    h = (sb.height + dy).max(min_h);
                }
                ResizeDir::S => {
                    y = sb.y + dy;
                    h = (sb.height - dy).max(min_h);
                    if h == min_h {
                        y = sb.y + sb.height - min_h;
                    }
                }
                ResizeDir::E => {
                    w = (sb.width + dx).max(MIN_W);
                }
                ResizeDir::W => {
                    x = sb.x + dx;
                    w = (sb.width - dx).max(MIN_W);
                    if w == MIN_W {
                        x = sb.x + sb.width - MIN_W;
                    }
                }
                ResizeDir::NE => {
                    w = (sb.width + dx).max(MIN_W);
                    h = (sb.height + dy).max(min_h);
                }
                ResizeDir::NW => {
                    x = sb.x + dx;
                    w = (sb.width - dx).max(MIN_W);
                    if w == MIN_W {
                        x = sb.x + sb.width - MIN_W;
                    }
                    h = (sb.height + dy).max(min_h);
                }
                ResizeDir::SE => {
                    w = (sb.width + dx).max(MIN_W);
                    y = sb.y + dy;
                    h = (sb.height - dy).max(min_h);
                    if h == min_h {
                        y = sb.y + sb.height - min_h;
                    }
                }
                ResizeDir::SW => {
                    x = sb.x + dx;
                    w = (sb.width - dx).max(MIN_W);
                    if w == MIN_W {
                        x = sb.x + sb.width - MIN_W;
                    }
                    y = sb.y + dy;
                    h = (sb.height - dy).max(min_h);
                    if h == min_h {
                        y = sb.y + sb.height - min_h;
                    }
                }
            }
        }

        self.bounds = snap_edges(x, y, w, h);
        self.clamp_to_canvas();
    }
}

/// Round a resized rectangle to whole units by its EDGES, not its size: the
/// edge that is not being dragged sits on a whole unit already (the drag
/// starts from snapped bounds), so it stays exactly where it was. Rounding
/// the left and the width separately moved the right edge a unit whenever the
/// mouse travel landed on a half unit, as a drag inside a zoomed container
/// does (a 5 px step at zoom 2 is 2.5 units).
fn snap_edges(x: f64, y: f64, w: f64, h: f64) -> Rect {
    let (left, bottom) = (x.round(), y.round());
    let (right, top) = ((x + w).round(), (y + h).round());
    Rect::new(left, bottom, right - left, top - bottom)
}
