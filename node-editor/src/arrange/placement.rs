//! Arrange stage 3 (see [`super`]): coordinates for the ordered layered
//! graph.
//!
//! x: each column starts at the previous column's x plus its widest card
//! plus the column gap; cards are left-aligned in their column.
//!
//! y: every column is first stacked tightly, then sweeps alternate
//! left → right and right → left. In a sweep each node wants the height
//! that makes its median noodle into the neighbouring column straight
//! (with several sockets, the median by socket order: the main input of
//! a two-input card is its first), and the column takes the closest
//! heights to those wishes that keep its order and the gaps — an exact
//! weighted least-squares fit, solved by pool-adjacent-violators after
//! shifting each node by the space the nodes above it need. Noodle lanes
//! weigh more than cards so long noodles stay straight. The last sweep
//! runs left → right, so a chain reads straight from the inputs on.

use super::ordering::Layered;
use super::{ArrangeOptions, Card, ComponentLayout};

/// Left → right plus right → left sweep pairs before the final one.
const ROUNDS: usize = 4;
/// How much more a noodle lane's wish counts than a card's.
const LANE_WEIGHT: f64 = 4.0;
/// Weight of a node with nothing to line up with: it stays put unless
/// its neighbours need the room.
const IDLE_WEIGHT: f64 = 0.125;

/// Place one ordered component: `(x, down)` for every card, `down`
/// measured from the top-most card's top.
pub(super) fn place(cards: &[Card], g: &Layered, opts: &ArrangeOptions) -> ComponentLayout {
    let total = g.w.len();
    let mut column_x = Vec::with_capacity(g.layers.len());
    let mut x = 0.0;
    for col in &g.layers {
        column_x.push(x);
        let widest = col.iter().map(|&v| g.w[v]).fold(0.0, f64::max);
        x = x + widest + opts.column_gap;
    }

    let mut tops = vec![0.0; total];
    for col in &g.layers {
        g.stack(col, &mut tops, opts.row_gap);
    }
    let columns = g.layers.len();
    for _ in 0..ROUNDS {
        for i in 1..columns {
            align_column(g, &mut tops, i, true, opts.row_gap);
        }
        for i in (0..columns.saturating_sub(1)).rev() {
            align_column(g, &mut tops, i, false, opts.row_gap);
        }
    }
    for i in 1..columns {
        align_column(g, &mut tops, i, true, opts.row_gap);
    }

    let mut places = vec![[0.0, 0.0]; cards.len()];
    let first_top = (0..cards.len())
        .map(|v| tops[v])
        .fold(f64::INFINITY, f64::min);
    let first_top = if first_top.is_finite() {
        first_top
    } else {
        0.0
    };
    let mut extent_top = 0.0f64;
    let mut extent_bottom = 0.0f64;
    for (i, col) in g.layers.iter().enumerate() {
        for &v in col {
            let down = tops[v] - first_top;
            extent_top = extent_top.min(down);
            extent_bottom = extent_bottom.max(down + g.h[v]);
            if v < cards.len() {
                places[v] = [column_x[i], down];
            }
        }
    }
    ComponentLayout {
        places,
        extent_top,
        extent_bottom,
    }
}

/// Re-place column `i` against the column to its left (`from_left`) or
/// right.
fn align_column(g: &Layered, tops: &mut [f64], i: usize, from_left: bool, row_gap: f64) {
    let col = &g.layers[i];
    let mut targets: Vec<f64> = Vec::with_capacity(col.len());
    let mut weights: Vec<f64> = Vec::with_capacity(col.len());
    let mut offsets: Vec<f64> = Vec::with_capacity(col.len());
    let mut space = 0.0;
    for (k, &v) in col.iter().enumerate() {
        if k > 0 {
            let p = col[k - 1];
            space = space + g.h[p] + g.separation(p, v, row_gap);
        }
        let segs = if from_left {
            &g.in_segs[v]
        } else {
            &g.out_segs[v]
        };
        // (own port offset, neighbour port height) per noodle.
        let mut ends: Vec<(f64, f64)> = segs
            .iter()
            .map(|&s| {
                let seg = &g.segs[s];
                if from_left {
                    (seg.right_off, tops[seg.left] + seg.left_off)
                } else {
                    (seg.left_off, tops[seg.right] + seg.right_off)
                }
            })
            .collect();
        let (wish, weight) = if ends.is_empty() {
            (tops[v], IDLE_WEIGHT)
        } else {
            ends.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
            let (own, other) = ends[(ends.len() - 1) / 2];
            let weight = if g.dummy[v] { LANE_WEIGHT } else { 1.0 };
            (other - own, weight)
        };
        offsets.push(space);
        targets.push(wish - space);
        weights.push(weight);
    }
    let fitted = isotonic_fit(&targets, &weights);
    for (k, &v) in col.iter().enumerate() {
        tops[v] = fitted[k] + offsets[k];
    }
}

/// Weighted least-squares fit of `targets` by a non-decreasing sequence
/// (pool-adjacent-violators): blocks that would go downward are merged
/// into their weighted mean, left to right.
fn isotonic_fit(targets: &[f64], weights: &[f64]) -> Vec<f64> {
    // (weight sum, weighted value sum, member count)
    let mut blocks: Vec<(f64, f64, usize)> = Vec::with_capacity(targets.len());
    for (&t, &w) in targets.iter().zip(weights) {
        blocks.push((w, w * t, 1));
        while blocks.len() > 1 {
            let (w1, s1, n1) = blocks[blocks.len() - 1];
            let (w0, s0, n0) = blocks[blocks.len() - 2];
            if s0 / w0 > s1 / w1 {
                blocks.pop();
                let last = blocks.len() - 1;
                blocks[last] = (w0 + w1, s0 + s1, n0 + n1);
            } else {
                break;
            }
        }
    }
    let mut out = Vec::with_capacity(targets.len());
    for (w, s, n) in blocks {
        let value = s / w;
        out.extend(std::iter::repeat_n(value, n));
    }
    out
}
