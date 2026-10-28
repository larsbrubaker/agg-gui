//! Automatic "Arrange" layout for node graphs — a pure module with no
//! widget or model trait, so any host can tidy a graph it describes.
//!
//! Inputs end up in the first column, outputs in the last, and every
//! other node in between, placed by connectivity: the layered method
//! (Sugiyama, Tagawa and Toda 1981; Gansner et al. 1993) in four stages,
//! one file each:
//!
//! - `layering`: inside one weakly connected component, edges are
//!   oriented left → right (cycles broken by a DFS) and every node gets a
//!   column.
//! - `ordering`: edges longer than one column get dummy nodes (their
//!   own lane, so a long noodle never runs through a card), and the order
//!   inside each column is found by barycenter sweeps that count crossings.
//! - `placement`: column x from the widest card, then y so the main
//!   noodle into each card runs straight, with no two cards closer than a
//!   gap.
//! - this file: the public description types, components (Input and
//!   Output cards with no noodles join the main one), stacking them
//!   top to bottom, and anchoring the result at the graph's current
//!   top-left so the view does not jump.
//!
//! Coordinates are the editor's canvas convention: a node's `position` is
//! its card's top-left corner with y up, so the card spans
//! `[y - height, y]`. Port offsets are measured down from the card's top.
//! [`crate::NodeEditor::arrange_plan`] builds this description from a live
//! editor.

mod layering;
mod ordering;
mod placement;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_layout;
#[cfg(test)]
mod tests_lone_roles;
#[cfg(test)]
mod tests_robust;

use std::collections::BTreeMap;

use crate::model::NodeId;

/// Where a node belongs in an arranged graph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ArrangeRole {
    /// A graph input: placed in the first column of its component.
    Input,
    /// A graph output: placed in the last column of its component.
    Output,
    /// Placed by its connections.
    #[default]
    Normal,
}

/// One node of the graph to arrange.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeNode {
    pub id: NodeId,
    /// Card `[width, height]` in canvas units.
    pub size: [f64; 2],
    pub role: ArrangeRole,
    /// Each input socket's offset down from the card's top, in socket order.
    pub input_ports: Vec<f64>,
    /// Each output socket's offset down from the card's top, in socket order.
    pub output_ports: Vec<f64>,
    /// Current top-left corner (y up).
    pub position: [f64; 2],
}

/// One edge: output socket `from.1` of node `from.0` feeds input socket
/// `to.1` of node `to.0`. A socket index with no port offset (out of
/// range) is treated as the middle of the card's edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArrangeEdge {
    pub from: (NodeId, usize),
    pub to: (NodeId, usize),
}

/// Spacing of an arranged graph, in canvas units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrangeOptions {
    /// Clear space between one column's widest card and the next column.
    pub column_gap: f64,
    /// Least clear space between two cards (or noodle lanes) in a column.
    pub row_gap: f64,
    /// Clear space between two separately laid-out components.
    pub component_gap: f64,
}

impl Default for ArrangeOptions {
    fn default() -> Self {
        Self {
            column_gap: 60.0,
            row_gap: 30.0,
            component_gap: 60.0,
        }
    }
}

/// A node as the stages see it: sanitised size, ports and position, its
/// index in the caller's slice kept for the result.
#[derive(Clone, Debug)]
pub(crate) struct Card {
    pub w: f64,
    pub h: f64,
    pub role: ArrangeRole,
    pub inputs: Vec<f64>,
    pub outputs: Vec<f64>,
    pub x: f64,
    pub top: f64,
    pub id: u64,
}

impl Card {
    /// The `[width, height]` the card is laid out at.
    #[cfg(test)]
    pub fn size(&self) -> [f64; 2] {
        [self.w, self.h]
    }

    /// Offset of input `i` down from the top; the edge middle when out of range.
    pub fn input_off(&self, i: usize) -> f64 {
        self.inputs.get(i).copied().unwrap_or(self.h * 0.5)
    }

    /// Offset of output `i` down from the top; the edge middle when out of range.
    pub fn output_off(&self, i: usize) -> f64 {
        self.outputs.get(i).copied().unwrap_or(self.h * 0.5)
    }
}

/// An edge between local card indices with its port offsets resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Link {
    pub from: usize,
    pub from_off: f64,
    pub to: usize,
    pub to_off: f64,
}

/// One component's arrangement: `(x, down)` per member card and per
/// noodle lane point, `down` measured downward from the component's
/// top-most card top, all in whole canvas units.
pub(crate) struct ComponentLayout {
    pub places: Vec<[f64; 2]>,
    pub lanes: Vec<[f64; 2]>,
}

/// Largest card size, port offset or gap taken as given, and largest
/// position: far beyond any real canvas, small enough that sums over a
/// few thousand cards stay finite.
const MAX_SIZE: f64 = 1e9;
const MAX_POSITION: f64 = 1e12;

/// Arrange the graph: the new top-left of every node, in the order of
/// `nodes` (a repeated id keeps its first entry only). Edges naming an
/// unknown node, self-loops and duplicates are ignored; an empty graph
/// gives an empty plan. Every position is in whole canvas units; the
/// result is deterministic, and arranging an arranged graph gives the
/// same positions again, also after a host's own arithmetic moved them by
/// a rounding error. Sizes, offsets and gaps that are not finite count as
/// 0 (an offset: the edge middle), negative ones as 0, and huge ones are
/// capped at a billion units.
pub fn arrange(
    nodes: &[ArrangeNode],
    edges: &[ArrangeEdge],
    options: &ArrangeOptions,
) -> Vec<(NodeId, [f64; 2])> {
    let opts = sanitize_options(options);
    let mut index: BTreeMap<u64, usize> = BTreeMap::new();
    let mut cards: Vec<Card> = Vec::new();
    let mut ids: Vec<NodeId> = Vec::new();
    for n in nodes {
        if index.contains_key(&n.id.0) {
            continue;
        }
        index.insert(n.id.0, cards.len());
        cards.push(sanitize_card(n));
        ids.push(n.id);
    }
    if cards.is_empty() {
        return Vec::new();
    }
    let links = resolve_links(&cards, &index, edges);
    let components = join_lone_roles(&cards, components(&cards, &links));

    // The anchor is rounded like every other output, so the result is in
    // whole canvas units.
    let anchor_x = cards
        .iter()
        .map(|c| c.x)
        .fold(f64::INFINITY, f64::min)
        .round_ties_even();
    let anchor_top = cards
        .iter()
        .map(|c| c.top)
        .fold(f64::NEG_INFINITY, f64::max)
        .round_ties_even();
    let mut result: Vec<[f64; 2]> = vec![[0.0, 0.0]; cards.len()];
    let mut offset = 0.0;
    let mut prev_bottom: Option<f64> = None;
    for members in &components {
        let local_of: BTreeMap<usize, usize> =
            members.iter().enumerate().map(|(l, &g)| (g, l)).collect();
        let local_cards: Vec<Card> = members.iter().map(|&g| cards[g].clone()).collect();
        let local_links: Vec<Link> = links
            .iter()
            .filter_map(|l| {
                let from = *local_of.get(&l.from)?;
                let to = *local_of.get(&l.to)?;
                Some(Link { from, to, ..*l })
            })
            .collect();
        let layout = layout_component(&local_cards, &local_links, &opts);
        // The vertical extent of everything drawn: cards and lanes.
        let mut extent_top = 0.0f64;
        let mut extent_bottom = 0.0f64;
        for (c, p) in local_cards.iter().zip(&layout.places) {
            extent_top = extent_top.min(p[1]);
            extent_bottom = extent_bottom.max(p[1] + c.h);
        }
        for p in &layout.lanes {
            extent_top = extent_top.min(p[1]);
            extent_bottom = extent_bottom.max(p[1]);
        }
        if let Some(bottom) = prev_bottom {
            offset = (bottom + opts.component_gap - extent_top).ceil();
        }
        for (l, &g) in members.iter().enumerate() {
            let [x, down] = layout.places[l];
            // `+ 0.0` turns a -0.0 into 0.0.
            result[g] = [anchor_x + x + 0.0, anchor_top - (offset + down) + 0.0];
        }
        prev_bottom = Some(offset + extent_bottom);
    }
    ids.into_iter().zip(result).collect()
}

/// Lay out one weakly connected component through the three stages.
fn layout_component(cards: &[Card], links: &[Link], opts: &ArrangeOptions) -> ComponentLayout {
    let oriented = layering::orient(cards, links);
    let layers = layering::assign_layers(cards, &oriented);
    let graph = ordering::order(cards, &oriented, &layers, opts);
    placement::place(cards, &graph, opts)
}

/// Gaps must be finite and not negative for the separation constraints.
fn sanitize_options(o: &ArrangeOptions) -> ArrangeOptions {
    let fix = |v: f64| {
        if v.is_finite() {
            v.clamp(0.0, MAX_SIZE)
        } else {
            0.0
        }
    };
    ArrangeOptions {
        column_gap: fix(o.column_gap),
        row_gap: fix(o.row_gap),
        component_gap: fix(o.component_gap),
    }
}

fn sanitize_card(n: &ArrangeNode) -> Card {
    let size = |v: f64| {
        if v.is_finite() {
            v.clamp(0.0, MAX_SIZE)
        } else {
            0.0
        }
    };
    // `+ 0.0` turns a -0.0 into 0.0.
    let at = |v: f64| {
        if v.is_finite() {
            v.clamp(-MAX_POSITION, MAX_POSITION) + 0.0
        } else {
            0.0
        }
    };
    let w = size(n.size[0]);
    let h = size(n.size[1]);
    // A port is clamped onto the card's edge so its height orders with
    // the card's neighbours in the column.
    let port = |v: &f64| {
        if v.is_finite() {
            v.clamp(0.0, h)
        } else {
            h * 0.5
        }
    };
    Card {
        w,
        h,
        role: n.role,
        inputs: n.input_ports.iter().map(port).collect(),
        outputs: n.output_ports.iter().map(port).collect(),
        x: at(n.position[0]),
        top: at(n.position[1]),
        id: n.id.0,
    }
}

/// Edges between known cards with ports resolved, self-loops dropped and
/// duplicates removed, sorted for a deterministic order.
fn resolve_links(cards: &[Card], index: &BTreeMap<u64, usize>, edges: &[ArrangeEdge]) -> Vec<Link> {
    let mut keyed: Vec<(usize, usize, usize, usize)> = edges
        .iter()
        .filter_map(|e| {
            let from = *index.get(&e.from.0 .0)?;
            let to = *index.get(&e.to.0 .0)?;
            (from != to).then_some((from, e.from.1, to, e.to.1))
        })
        .collect();
    keyed.sort_unstable();
    keyed.dedup();
    keyed
        .into_iter()
        .map(|(from, fp, to, tp)| Link {
            from,
            from_off: cards[from].output_off(fp),
            to,
            to_off: cards[to].input_off(tp),
        })
        .collect()
}

/// An Input or Output card with no noodles is a component of its own,
/// but the user still expects it at the left or right of the graph: it
/// joins the main component (the one with the most cards, then the
/// smallest node id — never a position, so a second arrange picks the
/// same one), where layering gives it the first or last column. With no
/// other component, the lone role cards form one component together.
/// The result is ordered again by the components' rule.
fn join_lone_roles(cards: &[Card], components: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
    let lone = |m: &Vec<usize>| m.len() == 1 && cards[m[0]].role != ArrangeRole::Normal;
    let (roles, mut rest): (Vec<Vec<usize>>, Vec<Vec<usize>>) =
        components.into_iter().partition(lone);
    if roles.is_empty() {
        return rest;
    }
    let smallest_id = |m: &Vec<usize>| m.iter().map(|&v| cards[v].id).min().unwrap_or(0);
    let main = (0..rest.len()).min_by(|&a, &b| {
        rest[b]
            .len()
            .cmp(&rest[a].len())
            .then(smallest_id(&rest[a]).cmp(&smallest_id(&rest[b])))
    });
    let joined: Vec<usize> = roles.into_iter().flatten().collect();
    match main {
        Some(i) => {
            rest[i].extend(joined);
            rest[i].sort_unstable();
        }
        None => {
            let mut all = joined;
            all.sort_unstable();
            rest.push(all);
        }
    }
    sort_components(cards, &mut rest);
    rest
}

/// Weakly connected components (card indices ascending inside each),
/// ordered top-most first by current position, then by smallest node id.
fn components(cards: &[Card], links: &[Link]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..cards.len()).collect();
    fn find(parent: &mut [usize], mut v: usize) -> usize {
        while parent[v] != v {
            parent[v] = parent[parent[v]];
            v = parent[v];
        }
        v
    }
    for l in links {
        let a = find(&mut parent, l.from);
        let b = find(&mut parent, l.to);
        if a != b {
            parent[a.max(b)] = a.min(b);
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for v in 0..cards.len() {
        let root = find(&mut parent, v);
        groups.entry(root).or_default().push(v);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    sort_components(cards, &mut out);
    out
}

/// Top-most component first by current position, then by smallest node id.
fn sort_components(cards: &[Card], out: &mut [Vec<usize>]) {
    let key = |m: &Vec<usize>| {
        let top = m
            .iter()
            .map(|&v| cards[v].top)
            .fold(f64::NEG_INFINITY, f64::max);
        let id = m.iter().map(|&v| cards[v].id).min().unwrap_or(0);
        (top, id)
    };
    out.sort_by(|a, b| {
        let (ta, ia) = key(a);
        let (tb, ib) = key(b);
        tb.total_cmp(&ta).then(ia.cmp(&ib))
    });
}
