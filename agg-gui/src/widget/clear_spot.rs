//! Where a box of a given size fits nearest to where it wants to be: pure
//! rectangle geometry for a card that floats over a view and must cover no
//! control, given the rectangles to keep clear of (from
//! [`painted_over`](super::painted_over), say).
//!
//! The search is exact, with no sampling. A box at lower-left `(x, y)`
//! overlaps an obstacle exactly while `(x, y)` lies inside the obstacle grown
//! by the box's size to the left and down (its Minkowski sum with the box),
//! and the area it covers of an obstacle is a product of two piecewise-linear
//! functions of `x` and of `y`, bent only where an edge of the box meets an
//! edge of the obstacle. So every grown edge, every bend and the region's own
//! limits split the plane into a grid of cells, inside each of which the
//! covered area is bilinear, and the clear positions are a union of cells,
//! cell edges and grid points. The best position (least covered area, then
//! nearest) therefore lies on that grid, or where one of its lines crosses the
//! ideal position's row or column (a projection of the ideal spot onto an
//! edge). Those crossings are the candidates; this includes the ideal spot
//! itself, its projections onto every grown edge, and every corner of a grown
//! obstacle and of the region, and also the points where one grown edge
//! crosses another, without which a spot squeezed between two obstacles would
//! be missed.
//!
//! Geometry is `f64` in a fixed order of operations, so the same inputs pick
//! the same spot on every platform.

use crate::geometry::{Point, Rect, Size};
use std::cmp::Ordering;

/// The box of `size` placed with its lower-left corner nearest `ideal`
/// (where that corner would like to sit), lying inside `region` and
/// overlapping no `hard` obstacle, covering as little of the `soft` obstacles
/// as can be. `None` exactly when no position inside `region`
/// keeps clear of every hard obstacle (the box larger than the region
/// included).
///
/// "Overlaps" means shares an area: a box touching an obstacle along an edge
/// keeps clear of it, and an obstacle with no area is never in the way. The
/// box lies inside the region when every edge of it is on or inside the
/// region's.
///
/// Among the clear positions it picks the one covering the least total area
/// of soft obstacles (summed in their order), then the nearest to `ideal`
/// (Euclidean), then, between equally near ones, the higher (larger `y`),
/// then the one further left (smaller `x`).
///
/// The box's far edges are computed as `x + width` and `y + height`; a box
/// placed against an obstacle's near edge is moved down to the largest
/// coordinate whose sum stays on that edge, so the box that comes back keeps
/// clear by that same sum.
pub fn nearest_clear_spot(
    region: Rect,
    hard: &[Rect],
    soft: &[Rect],
    size: Size,
    ideal: Point,
) -> Option<Rect> {
    let (w, h) = (size.width, size.height);
    if !(w >= 0.0 && h >= 0.0) {
        return None;
    }
    let (min_x, min_y) = (region.left(), region.bottom());
    let max_x = fit_below(region.right(), w);
    let max_y = fit_below(region.top(), h);
    if !(min_x <= max_x && min_y <= max_y) {
        return None;
    }
    let hard: Vec<Rect> = hard.iter().copied().filter(has_area).collect();
    let soft: Vec<Rect> = soft.iter().copied().filter(has_area).collect();

    let xs = candidates(ideal.x, min_x, max_x, w, &hard, &soft, Axis::X);
    let ys = candidates(ideal.y, min_y, max_y, h, &hard, &soft, Axis::Y);

    let mut best: Option<Best> = None;
    let mut blocking: Vec<usize> = Vec::with_capacity(hard.len());
    let mut soft_x: Vec<(usize, f64)> = Vec::with_capacity(soft.len());
    for &x in &xs {
        // The obstacles this column already overlaps sideways: only they can
        // block it or be covered by it, whatever the height.
        blocking.clear();
        blocking
            .extend((0..hard.len()).filter(|&i| span(x, w, hard[i].left(), hard[i].right()) > 0.0));
        soft_x.clear();
        soft_x.extend((0..soft.len()).filter_map(|i| {
            let along = span(x, w, soft[i].left(), soft[i].right());
            (along > 0.0).then_some((i, along))
        }));
        for &y in &ys {
            let blocked = blocking
                .iter()
                .any(|&i| span(y, h, hard[i].bottom(), hard[i].top()) > 0.0);
            if blocked {
                continue;
            }
            let mut covered = 0.0;
            for &(i, along) in &soft_x {
                covered += along * span(y, h, soft[i].bottom(), soft[i].top());
            }
            let dx = x - ideal.x;
            let dy = y - ideal.y;
            let candidate = Best {
                covered,
                distance_sq: dx * dx + dy * dy,
                x,
                y,
            };
            if best.is_none_or(|b| candidate.better_than(&b)) {
                best = Some(candidate);
            }
        }
    }
    best.map(|b| Rect::new(b.x, b.y, w, h))
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

/// The candidate coordinates along one axis: the ideal one, the region's
/// limits, every grown obstacle edge and every bend of the covered area,
/// kept inside `[min, max]`, sorted and without repeats.
fn candidates(
    ideal: f64,
    min: f64,
    max: f64,
    extent: f64,
    hard: &[Rect],
    soft: &[Rect],
    axis: Axis,
) -> Vec<f64> {
    let low_high = |r: &Rect| match axis {
        Axis::X => (r.left(), r.right()),
        Axis::Y => (r.bottom(), r.top()),
    };
    let mut out = Vec::with_capacity(3 + 2 * hard.len() + 4 * soft.len());
    out.extend([ideal, min, max]);
    for r in hard {
        let (low, high) = low_high(r);
        // The box's far edge against the obstacle's near one, and its near
        // edge against the far one.
        out.extend([fit_below(low, extent), high]);
    }
    for r in soft {
        let (low, high) = low_high(r);
        // Coverage starts, stops growing at one edge, starts shrinking at the
        // other, and ends.
        out.extend([fit_below(low, extent), low, fit_below(high, extent), high]);
    }
    out.retain(|v| min <= *v && *v <= max);
    out.sort_by(f64::total_cmp);
    out.dedup();
    out
}

/// The largest `start` with `start + extent <= edge`: the coordinate that
/// puts a box's far edge on `edge` as `start + extent` computes it.
/// `edge - extent` alone can round to a box reaching an ulp past `edge`.
fn fit_below(edge: f64, extent: f64) -> f64 {
    let mut start = edge - extent;
    while start + extent > edge {
        start = start.next_down();
    }
    start
}

/// How far the box's extent `[start, start + extent]` overlaps `[low, high]`
/// along one axis; zero when they only touch or miss.
///
/// Decided by where the box's own edges (`start` and `start + extent`, as the
/// result reports them) fall, not by `min(end, high) - max(start, low)`: a
/// box wholly inside the obstacle then overlaps it by exactly `extent`, and
/// one wholly over it by exactly the obstacle's length, wherever it sits, so
/// the covered area does not jitter by an ulp from place to place (a box
/// placed against the obstacle's far edge measured an ulp short) and
/// outrank the distance to the ideal spot.
fn span(start: f64, extent: f64, low: f64, high: f64) -> f64 {
    let end = start + extent;
    if end <= low || start >= high {
        return 0.0;
    }
    match (start >= low, end <= high) {
        (true, true) => extent,
        (false, false) => high - low,
        (true, false) => high - start,
        (false, true) => end - low,
    }
}

fn has_area(r: &Rect) -> bool {
    r.width > 0.0 && r.height > 0.0
}

/// A clear position and what ranks it.
#[derive(Clone, Copy)]
struct Best {
    covered: f64,
    distance_sq: f64,
    x: f64,
    y: f64,
}

impl Best {
    /// Less covered, then nearer, then higher, then further left.
    fn better_than(&self, other: &Best) -> bool {
        let order = self
            .covered
            .total_cmp(&other.covered)
            .then(self.distance_sq.total_cmp(&other.distance_sq))
            .then(other.y.total_cmp(&self.y))
            .then(self.x.total_cmp(&other.x));
        order == Ordering::Less
    }
}
