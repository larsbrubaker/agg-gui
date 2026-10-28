//! Arrange stage 2 (see [`super`]): the layered graph and the order of
//! the cards inside each column.
//!
//! An edge spanning several columns is split into one segment per column
//! gap through dummy nodes — zero-size points that become the noodle's
//! own lane — so placement keeps cards out of its way.
//!
//! The order of the cards in each column starts from their current
//! heights (the user's arrangement is respected); a lane point always
//! goes where its noodle's straight line crosses the column
//! ([`canonical`]). Barycenter sweeps then alternate left → right and
//! right → left: each node moves to the mean height of the ports it
//! connects to in the neighbouring column, so the feed into a card's
//! socket 0 sits above the feed into its socket 1. After every sweep the
//! cards' order, lanes put back on their lines, is counted for crossings;
//! the fewest seen is kept, the starting order winning ties, and sweeping
//! restarts from it until a round finds nothing better. Every sort breaks
//! ties by the previous rank, and the first order by node index.

use super::{ArrangeOptions, Card, Link};

/// Sweeps per series; a series starts from the best order so far.
const SWEEPS: usize = 8;
/// Most rounds of two series (left → right first, right → left first).
const ROUNDS: usize = 6;

/// One noodle segment between neighbouring columns: `left` in column `i`,
/// `right` in column `i + 1`, with port offsets down from each top.
#[derive(Clone, Copy, Debug)]
pub(super) struct Seg {
    pub left: usize,
    pub left_off: f64,
    pub right: usize,
    pub right_off: f64,
}

/// The layered graph: cards `0..cards.len()`, dummy lane points after.
pub(super) struct Layered {
    pub w: Vec<f64>,
    pub h: Vec<f64>,
    pub dummy: Vec<bool>,
    /// Column of every node.
    pub layer: Vec<usize>,
    /// For a lane point, its whole noodle and how far along it the point
    /// lies (column steps over the noodle's span).
    pub lane: Vec<Option<(Link, f64)>>,
    /// Every column top to bottom.
    pub layers: Vec<Vec<usize>>,
    pub segs: Vec<Seg>,
    /// Segments arriving at each node from the column to its left.
    pub in_segs: Vec<Vec<usize>>,
    /// Segments leaving each node to the column to its right.
    pub out_segs: Vec<Vec<usize>>,
    /// Segments between column `i` and `i + 1`.
    pub gap_segs: Vec<Vec<usize>>,
}

impl Layered {
    /// Least clear space between neighbours `a` above `b` in a column: two
    /// noodle lanes sit closer than anything next to a card.
    pub fn separation(&self, a: usize, b: usize, row_gap: f64) -> f64 {
        if self.dummy[a] && self.dummy[b] {
            row_gap * 0.5
        } else {
            row_gap
        }
    }

    /// Stack column `layer` from height 0 down, each node `separation`
    /// below the previous one's bottom.
    pub fn stack(&self, layer: &[usize], tops: &mut [f64], row_gap: f64) {
        let mut next = 0.0;
        let mut prev: Option<usize> = None;
        for &v in layer {
            if let Some(p) = prev {
                next = tops[p] + self.h[p] + self.separation(p, v, row_gap);
            }
            tops[v] = next;
            prev = Some(v);
        }
    }
}

/// Build the layered graph and choose the order inside every column.
pub(super) fn order(
    cards: &[Card],
    links: &[Link],
    layer_of: &[usize],
    opts: &ArrangeOptions,
) -> Layered {
    let n = cards.len();
    let columns = layer_of.iter().copied().max().map_or(0, |m| m + 1);
    let mut g = Layered {
        w: cards.iter().map(|c| c.w).collect(),
        h: cards.iter().map(|c| c.h).collect(),
        dummy: vec![false; n],
        layer: layer_of.to_vec(),
        lane: vec![None; n],
        layers: vec![Vec::new(); columns],
        segs: Vec::new(),
        in_segs: vec![Vec::new(); n],
        out_segs: vec![Vec::new(); n],
        gap_segs: vec![Vec::new(); columns.saturating_sub(1)],
    };
    for l in links {
        let a = layer_of[l.from];
        let span = layer_of[l.to] - a;
        let (mut prev, mut prev_off) = (l.from, l.from_off);
        for k in 1..=span {
            let (next, next_off) = if k == span {
                (l.to, l.to_off)
            } else {
                let d = g.w.len();
                g.w.push(0.0);
                g.h.push(0.0);
                g.dummy.push(true);
                g.layer.push(a + k);
                g.lane.push(Some((*l, (k as f64) / (span as f64))));
                g.in_segs.push(Vec::new());
                g.out_segs.push(Vec::new());
                (d, 0.0)
            };
            let s = g.segs.len();
            g.segs.push(Seg {
                left: prev,
                left_off: prev_off,
                right: next,
                right_off: next_off,
            });
            g.out_segs[prev].push(s);
            g.in_segs[next].push(s);
            g.gap_segs[a + k - 1].push(s);
            prev = next;
            prev_off = next_off;
        }
    }
    // The user's arrangement: cards top to bottom by their current top.
    let mut cards_order: Vec<Vec<usize>> = vec![Vec::new(); columns];
    for v in 0..n {
        cards_order[layer_of[v]].push(v);
    }
    for col in &mut cards_order {
        col.sort_by(|&a, &b| cards[b].top.total_cmp(&cards[a].top).then(a.cmp(&b)));
    }
    g.layers = canonical(&g, &cards_order, opts.row_gap);

    let mut tops = vec![0.0; g.w.len()];
    for col in &g.layers {
        g.stack(col, &mut tops, opts.row_gap);
    }
    let mut best = g.layers.clone();
    let mut best_crossings = crossings(&g, &tops);
    let mut trial = g.layers.clone();
    let mut trial_tops = tops.clone();
    // Sweep again from the best order until a round finds nothing better,
    // so the order kept is one the sweeps cannot improve: arranging an
    // arranged graph then starts from it (its cards' order is read back
    // from their heights, the lanes put back by `canonical`) and keeps it.
    for _ in 0..ROUNDS {
        if best_crossings == 0 {
            break;
        }
        let start = best.clone();
        let mut improved = false;
        for first_down in [true, false] {
            g.layers = start.clone();
            for col in &g.layers {
                g.stack(col, &mut tops, opts.row_gap);
            }
            for sweep in 0..SWEEPS {
                let down = (sweep % 2 == 0) == first_down;
                sweep_once(&mut g, &mut tops, down, opts.row_gap);
                let cards_only: Vec<Vec<usize>> = g
                    .layers
                    .iter()
                    .map(|col| col.iter().copied().filter(|&v| !g.dummy[v]).collect())
                    .collect();
                trial.clone_from(&canonical(&g, &cards_only, opts.row_gap));
                for col in &trial {
                    g.stack(col, &mut trial_tops, opts.row_gap);
                }
                let c = crossings(&g, &trial_tops);
                if c < best_crossings {
                    best_crossings = c;
                    best.clone_from(&trial);
                    improved = true;
                }
                if best_crossings == 0 {
                    break;
                }
            }
        }
        if !improved {
            break;
        }
    }
    g.layers = best;
    g
}

/// The full column order for a given order of the cards: each noodle
/// lane point goes where the straight line between its noodle's two
/// ports crosses its column, with every column stacked from the cards
/// alone. It depends on the cards' order only, so an arranged graph's
/// order can be rebuilt exactly from its cards' heights.
fn canonical(g: &Layered, cards_order: &[Vec<usize>], row_gap: f64) -> Vec<Vec<usize>> {
    let mut tops = vec![0.0; g.w.len()];
    for col in cards_order {
        g.stack(col, &mut tops, row_gap);
    }
    let mut keyed: Vec<Vec<(f64, usize)>> = cards_order
        .iter()
        .map(|col| col.iter().map(|&v| (tops[v] + g.h[v] * 0.5, v)).collect())
        .collect();
    for (v, lane) in g.lane.iter().enumerate() {
        if let Some((l, t)) = lane {
            let start = tops[l.from] + l.from_off;
            let end = tops[l.to] + l.to_off;
            keyed[g.layer[v]].push((start + (end - start) * t, v));
        }
    }
    keyed
        .into_iter()
        .map(|mut col| {
            // Cards come before lane points in index order, so a lane
            // level with a card's middle goes below it.
            col.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            col.into_iter().map(|(_, v)| v).collect()
        })
        .collect()
}

/// One barycenter sweep: left → right when `down`, else right → left.
fn sweep_once(g: &mut Layered, tops: &mut [f64], down: bool, row_gap: f64) {
    let columns = g.layers.len();
    let order: Vec<usize> = if down {
        (1..columns).collect()
    } else {
        (0..columns.saturating_sub(1)).rev().collect()
    };
    for i in order {
        let mut col = std::mem::take(&mut g.layers[i]);
        let keys: Vec<(f64, usize)> = col
            .iter()
            .enumerate()
            .map(|(rank, &v)| {
                let segs = if down { &g.in_segs[v] } else { &g.out_segs[v] };
                let mut sum = 0.0;
                for &s in segs {
                    let seg = &g.segs[s];
                    sum += if down {
                        tops[seg.left] + seg.left_off
                    } else {
                        tops[seg.right] + seg.right_off
                    };
                }
                // A node with nothing in the neighbouring column keeps its
                // own height, so it stays roughly where it is.
                let k = if segs.is_empty() {
                    tops[v]
                } else {
                    sum / (segs.len() as f64)
                };
                (k, rank)
            })
            .collect();
        let mut ranked: Vec<(f64, usize, usize)> = keys
            .iter()
            .zip(col.iter())
            .map(|(&(k, r), &v)| (k, r, v))
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        col = ranked.into_iter().map(|(_, _, v)| v).collect();
        g.stack(&col, tops, row_gap);
        g.layers[i] = col;
    }
}

/// Noodle crossings of the whole layered graph with nodes at `tops`. Two
/// segments cross when their ends swap order between the two columns;
/// segments sharing a port at either end never count.
pub(super) fn crossings(g: &Layered, tops: &[f64]) -> u64 {
    let mut total = 0u64;
    for segs in &g.gap_segs {
        let mut ends: Vec<(f64, f64)> = segs
            .iter()
            .map(|&s| {
                let seg = &g.segs[s];
                (
                    tops[seg.left] + seg.left_off,
                    tops[seg.right] + seg.right_off,
                )
            })
            .collect();
        ends.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut rights: Vec<f64> = ends.into_iter().map(|e| e.1).collect();
        total += inversions(&mut rights);
    }
    total
}

/// Pairs `i < j` with `v[i] > v[j]`, counted by merge sort (sorts `v`).
fn inversions(v: &mut [f64]) -> u64 {
    if v.len() < 2 {
        return 0;
    }
    let mid = v.len() / 2;
    let mut count = inversions(&mut v[..mid]) + inversions(&mut v[mid..]);
    let mut merged = Vec::with_capacity(v.len());
    let (mut i, mut j) = (0, mid);
    while i < mid && j < v.len() {
        if v[j] < v[i] {
            count += (mid - i) as u64;
            merged.push(v[j]);
            j += 1;
        } else {
            merged.push(v[i]);
            i += 1;
        }
    }
    merged.extend_from_slice(&v[i..mid]);
    merged.extend_from_slice(&v[j..]);
    v.copy_from_slice(&merged);
    count
}
