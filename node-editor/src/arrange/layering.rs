//! Arrange stage 1 (see [`super`]): orient every edge of one component
//! left → right and give each card a column.
//!
//! Orientation honours the roles first (nothing may feed into an Input or
//! out of an Output, so such an edge is turned around for layout only),
//! then breaks any cycle with a depth-first search whose back edges are
//! turned around. Columns come from longest-path layering; Outputs are
//! then moved to the component's last column, and a card nothing feeds
//! (a constant, a value) is pulled right to sit just left of its nearest
//! consumer rather than piling up in column 0. Input and Output cards
//! with no noodles take the first and last columns (`place_lone_roles`).

use std::collections::BTreeSet;

use super::{ArrangeRole, Card, Link};

/// The component's links oriented for layout. A link both of whose ends
/// are Inputs (or both Outputs) cannot run left → right between their
/// fixed columns, so it is left out of the layout.
pub(super) fn orient(cards: &[Card], links: &[Link]) -> Vec<Link> {
    let mut out: Vec<Link> = Vec::with_capacity(links.len());
    for l in links {
        let (a, b) = (cards[l.from].role, cards[l.to].role);
        if b == ArrangeRole::Input || a == ArrangeRole::Output {
            if a == ArrangeRole::Input || b == ArrangeRole::Output {
                continue;
            }
            out.push(reversed(l));
        } else {
            out.push(*l);
        }
    }
    break_cycles(cards, &mut out);
    out
}

fn reversed(l: &Link) -> Link {
    Link {
        from: l.to,
        from_off: l.to_off,
        to: l.from,
        to_off: l.from_off,
    }
}

/// Turn around every back edge of a deterministic depth-first search.
/// Roots are tried Inputs first, then cards nothing feeds, then the rest,
/// each by node id. Nothing here reads positions: arranging moves cards,
/// and a choice that followed them would turn different edges around the
/// second time, so an arranged graph would not stay put. Inputs
/// have no incoming edges and Outputs no outgoing ones, so neither can be
/// reached by a back edge and the roles stay satisfied.
fn break_cycles(cards: &[Card], links: &mut [Link]) {
    let n = cards.len();
    let mut out_edges: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indegree = vec![0usize; n];
    for (i, l) in links.iter().enumerate() {
        out_edges[l.from].push(i);
        indegree[l.to] += 1;
    }
    let mut roots: Vec<usize> = (0..n).collect();
    roots.sort_by(|&a, &b| {
        let rank = |v: usize| match (cards[v].role, indegree[v]) {
            (ArrangeRole::Input, _) => 0u8,
            (_, 0) => 1,
            _ => 2,
        };
        rank(a).cmp(&rank(b)).then(cards[a].id.cmp(&cards[b].id))
    });
    // 0 unvisited, 1 on the DFS stack, 2 finished.
    let mut state = vec![0u8; n];
    let mut flip: Vec<usize> = Vec::new();
    for root in roots {
        if state[root] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        state[root] = 1;
        while let Some(top) = stack.last_mut() {
            let (v, next) = *top;
            if let Some(&e) = out_edges[v].get(next) {
                top.1 += 1;
                let w = links[e].to;
                match state[w] {
                    0 => {
                        state[w] = 1;
                        stack.push((w, 0));
                    }
                    1 => flip.push(e),
                    _ => {}
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
    for e in flip {
        links[e] = reversed(&links[e]);
    }
}

/// Column of every card: 0 for the left-most, every oriented link going
/// from a lower column to a higher one.
pub(super) fn assign_layers(cards: &[Card], links: &[Link]) -> Vec<usize> {
    let n = cards.len();
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); n];
    for l in links {
        succs[l.from].push(l.to);
        preds[l.to].push(l.from);
    }
    // Longest path from the sources, in a topological order (Kahn's
    // algorithm, smallest index first). The graph is acyclic after
    // `orient`; a card a cycle somehow left unvisited keeps column 0.
    let mut layer = vec![0usize; n];
    let mut remaining: Vec<usize> = preds.iter().map(Vec::len).collect();
    let mut ready: BTreeSet<usize> = (0..n).filter(|&v| remaining[v] == 0).collect();
    let mut topo: Vec<usize> = Vec::with_capacity(n);
    while let Some(v) = ready.pop_first() {
        topo.push(v);
        for &w in &succs[v] {
            layer[w] = layer[w].max(layer[v] + 1);
            remaining[w] -= 1;
            if remaining[w] == 0 {
                ready.insert(w);
            }
        }
    }
    let last = layer.iter().copied().max().unwrap_or(0);
    for v in 0..n {
        if cards[v].role == ArrangeRole::Output {
            layer[v] = last;
        }
    }
    // Pull sources that are not Inputs right, next to their nearest
    // consumer. Their consumers' columns are final already: only sources
    // move here, and a source feeds no other source.
    for v in 0..n {
        if cards[v].role != ArrangeRole::Input && preds[v].is_empty() {
            if let Some(nearest) = succs[v].iter().map(|&w| layer[w]).min() {
                layer[v] = nearest.saturating_sub(1).max(layer[v]);
            }
        }
    }
    let first = layer.iter().copied().min().unwrap_or(0);
    for l in &mut layer {
        *l -= first;
    }
    place_lone_roles(cards, &preds, &succs, &mut layer);
    layer
}

/// Input and Output cards with no noodles (see `join_lone_roles`): the
/// Inputs take the first column — one of their own unless a wired Input
/// is there already — and the Outputs the last, likewise, so the graph
/// always reads from its inputs on the left to its outputs on the right.
fn place_lone_roles(
    cards: &[Card],
    preds: &[Vec<usize>],
    succs: &[Vec<usize>],
    layer: &mut [usize],
) {
    let n = cards.len();
    let lone = |v: usize, role: ArrangeRole| {
        cards[v].role == role && preds[v].is_empty() && succs[v].is_empty()
    };
    let lone_input = |v: usize| lone(v, ArrangeRole::Input);
    let lone_output = |v: usize| lone(v, ArrangeRole::Output);
    if (0..n).any(lone_input) {
        let wired_input = (0..n).any(|v| cards[v].role == ArrangeRole::Input && !lone_input(v));
        if !wired_input {
            for v in (0..n).filter(|&v| !lone_input(v)) {
                layer[v] += 1;
            }
        }
        for v in (0..n).filter(|&v| lone_input(v)) {
            layer[v] = 0;
        }
    }
    let wired_output = (0..n).any(|v| cards[v].role == ArrangeRole::Output && !lone_output(v));
    let rest_last = (0..n).filter(|&v| !lone_output(v)).map(|v| layer[v]).max();
    let column = match rest_last {
        Some(last) if wired_output => last,
        Some(last) => last + 1,
        None => 0,
    };
    for v in (0..n).filter(|&v| lone_output(v)) {
        layer[v] = column;
    }
}
