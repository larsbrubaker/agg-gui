//! Hit areas for the on-screen keyboard: every point of the panel belongs
//! to a key.
//!
//! The drawn keys have gaps between them, but a finger aimed at a key often
//! lands in a gap or past the last key of a row; the phone's own keyboard
//! types the nearest key there instead of nothing. [`assign_row_hits`]
//! grows each drawn key to the midlines between it and its neighbours,
//! each row to the midlines between rows, and the outermost keys and rows
//! to the panel's edges, so the hit areas tile the panel. [`key_at`] then
//! finds the owner of a point, falling back to the nearest drawn key for a
//! point no area covers (the panel's rounding at its very edge).
//!
//! Used by [`super::layouts::Layout::paint`], which records each key's hit
//! area in [`super::key::PaintedKey::hit`], and by [`super::pointer`].

use crate::geometry::{Point, Rect};

use super::key::PaintedKey;

/// The hit areas for one keyboard's rows of drawn keys (`rows[r][i]`, top
/// row first, keys left to right, Y-up rects), inside `area`: the tiling
/// described in the module comment. The result has the same shape as
/// `rows`.
pub(super) fn assign_row_hits(rows: &[Vec<Rect>], area: Rect) -> Vec<Vec<Rect>> {
    let area_top = area.y + area.height;
    let area_right = area.x + area.width;
    let last_row = rows.len().saturating_sub(1);
    rows.iter()
        .enumerate()
        .map(|(r, keys)| {
            // Rows are uniform in height, so the first key speaks for its row.
            let Some(first) = keys.first() else {
                return Vec::new();
            };
            let row_top = first.y + first.height;
            let top = if r == 0 {
                area_top
            } else {
                match rows[r - 1].first() {
                    Some(above) => (above.y + row_top) / 2.0,
                    None => area_top,
                }
            };
            let bottom = if r == last_row {
                area.y
            } else {
                match rows[r + 1].first() {
                    Some(below) => (first.y + below.y + below.height) / 2.0,
                    None => area.y,
                }
            };
            let last_key = keys.len() - 1;
            keys.iter()
                .enumerate()
                .map(|(i, k)| {
                    let left = if i == 0 {
                        area.x
                    } else {
                        let prev = keys[i - 1];
                        (prev.x + prev.width + k.x) / 2.0
                    };
                    let right = if i == last_key {
                        area_right
                    } else {
                        (k.x + k.width + keys[i + 1].x) / 2.0
                    };
                    Rect::new(left, bottom, right - left, top - bottom)
                })
                .collect()
        })
        .collect()
}

/// The index of the key that owns `pos`: the first whose hit area contains
/// it, else the key whose drawn face is nearest. `None` only when there are
/// no keys.
pub(super) fn key_at(keys: &[PaintedKey], pos: Point) -> Option<usize> {
    if let Some(i) = keys.iter().position(|k| k.hit.contains(pos)) {
        return Some(i);
    }
    keys.iter()
        .enumerate()
        .map(|(i, k)| (i, distance_sq(k.rect, pos)))
        // Ties go to the earlier key, so the answer never depends on
        // float noise in the comparison.
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(i, _)| i)
}

/// Squared distance from `pos` to the nearest point of `r` (0 inside).
fn distance_sq(r: Rect, pos: Point) -> f64 {
    let dx = (r.x - pos.x).max(0.0).max(pos.x - (r.x + r.width));
    let dy = (r.y - pos.y).max(0.0).max(pos.y - (r.y + r.height));
    dx * dx + dy * dy
}
