//! Tests for [`crate::arrange`]: the rules a tidied graph must keep (no
//! overlaps, edges left → right, roles in the end columns, sources next
//! to their consumer, straight chains) plus the shared helpers the
//! neighbouring `tests_layout` module uses.

use std::collections::BTreeMap;

use super::*;

pub(super) const W: f64 = 100.0;

pub(super) fn node(
    id: u64,
    role: ArrangeRole,
    h: f64,
    ins: &[f64],
    outs: &[f64],
    pos: [f64; 2],
) -> ArrangeNode {
    ArrangeNode {
        id: NodeId(id),
        size: [W, h],
        role,
        input_ports: ins.to_vec(),
        output_ports: outs.to_vec(),
        position: pos,
    }
}

pub(super) fn edge(a: u64, ap: usize, b: u64, bp: usize) -> ArrangeEdge {
    ArrangeEdge {
        from: (NodeId(a), ap),
        to: (NodeId(b), bp),
    }
}

pub(super) fn run(nodes: &[ArrangeNode], edges: &[ArrangeEdge]) -> BTreeMap<u64, [f64; 2]> {
    arrange(nodes, edges, &ArrangeOptions::default())
        .into_iter()
        .map(|(id, p)| (id.0, p))
        .collect()
}

/// `nodes` moved to `plan`.
pub(super) fn applied(nodes: &[ArrangeNode], plan: &BTreeMap<u64, [f64; 2]>) -> Vec<ArrangeNode> {
    nodes
        .iter()
        .map(|n| ArrangeNode {
            position: plan[&n.id.0],
            ..n.clone()
        })
        .collect()
}

/// Column index of every node: the rank of its distinct x.
pub(super) fn columns(plan: &BTreeMap<u64, [f64; 2]>) -> BTreeMap<u64, usize> {
    let mut xs: Vec<f64> = plan.values().map(|p| p[0]).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    plan.iter()
        .map(|(&id, p)| (id, xs.iter().position(|&x| x == p[0]).unwrap()))
        .collect()
}

/// Every pair of cards is a column gap apart side by side or a row gap
/// apart one above the other.
pub(super) fn assert_no_overlap(nodes: &[ArrangeNode], plan: &BTreeMap<u64, [f64; 2]>) {
    let o = ArrangeOptions::default();
    for (i, a) in nodes.iter().enumerate() {
        for b in &nodes[i + 1..] {
            let (pa, pb) = (plan[&a.id.0], plan[&b.id.0]);
            let x_gap = (pb[0] - (pa[0] + a.size[0])).max(pa[0] - (pb[0] + b.size[0]));
            let y_gap = ((pb[1] - b.size[1]) - pa[1]).max((pa[1] - a.size[1]) - pb[1]);
            assert!(
                x_gap >= o.column_gap - 1e-9 || y_gap >= o.row_gap - 1e-9,
                "cards {} and {} overlap: {pa:?} {pb:?}",
                a.id.0,
                b.id.0
            );
        }
    }
}

/// Canvas height of a port: the card's top minus its offset.
pub(super) fn port_y(
    nodes: &[ArrangeNode],
    plan: &BTreeMap<u64, [f64; 2]>,
    id: u64,
    output: bool,
    i: usize,
) -> f64 {
    let n = nodes.iter().find(|n| n.id.0 == id).unwrap();
    let ports = if output {
        &n.output_ports
    } else {
        &n.input_ports
    };
    plan[&id][1] - ports[i]
}

/// Crossings between edges that run between the same two columns.
pub(super) fn crossing_count(
    nodes: &[ArrangeNode],
    edges: &[ArrangeEdge],
    plan: &BTreeMap<u64, [f64; 2]>,
) -> usize {
    let cols = columns(plan);
    let ends: Vec<(usize, usize, f64, f64)> = edges
        .iter()
        .map(|e| {
            (
                cols[&e.from.0 .0],
                cols[&e.to.0 .0],
                port_y(nodes, plan, e.from.0 .0, true, e.from.1),
                port_y(nodes, plan, e.to.0 .0, false, e.to.1),
            )
        })
        .collect();
    let mut count = 0;
    for (i, a) in ends.iter().enumerate() {
        for b in &ends[i + 1..] {
            if a.0 == b.0 && a.1 == b.1 && (a.2 - b.2) * (a.3 - b.3) < 0.0 {
                count += 1;
            }
        }
    }
    count
}

/// Input → A → B → Output, each with its own height and socket heights,
/// scattered about the canvas.
pub(super) fn chain() -> (Vec<ArrangeNode>, Vec<ArrangeEdge>) {
    use ArrangeRole::*;
    let nodes = vec![
        node(1, Input, 60.0, &[], &[40.0], [500.0, 0.0]),
        node(2, Normal, 120.0, &[30.0, 70.0], &[90.0], [0.0, 300.0]),
        node(3, Normal, 80.0, &[50.0], &[35.0], [250.0, -200.0]),
        node(4, Output, 150.0, &[100.0], &[], [-300.0, 100.0]),
    ];
    let edges = vec![edge(1, 0, 2, 0), edge(2, 0, 3, 0), edge(3, 0, 4, 0)];
    (nodes, edges)
}

#[test]
fn empty_graph_gives_an_empty_plan() {
    assert!(arrange(&[], &[], &ArrangeOptions::default()).is_empty());
    assert!(arrange(&[], &[edge(1, 0, 2, 0)], &ArrangeOptions::default()).is_empty());
}

#[test]
fn a_single_node_stays_where_it_is() {
    let n = [node(7, ArrangeRole::Normal, 50.0, &[], &[], [12.5, -40.0])];
    assert_eq!(run(&n, &[])[&7], [12.5, -40.0]);
}

#[test]
fn a_chain_reads_left_to_right_with_straight_noodles() {
    let (nodes, edges) = chain();
    let plan = run(&nodes, &edges);
    let cols = columns(&plan);
    assert_eq!([cols[&1], cols[&2], cols[&3], cols[&4]], [0, 1, 2, 3]);
    for e in &edges {
        let from = port_y(&nodes, &plan, e.from.0 .0, true, e.from.1);
        let to = port_y(&nodes, &plan, e.to.0 .0, false, e.to.1);
        assert!(
            (from - to).abs() < 1e-9,
            "noodle {e:?} bends: {from} -> {to}"
        );
    }
    assert_no_overlap(&nodes, &plan);
    // Each column starts a column gap past the previous one's card.
    let o = ArrangeOptions::default();
    assert_eq!(plan[&2][0] - plan[&1][0], W + o.column_gap);
}

#[test]
fn the_arranged_graph_keeps_its_top_left_corner() {
    let (nodes, edges) = chain();
    let plan = run(&nodes, &edges);
    let left = plan.values().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let top = plan
        .values()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max);
    assert_eq!([left, top], [-300.0, 300.0]);
}

#[test]
fn inputs_go_first_and_outputs_last_whatever_the_edges_say() {
    use ArrangeRole::*;
    // An edge into the Input and one out of the Output are laid out
    // turned around; a lone Output and Input still take the end columns.
    let nodes = vec![
        node(1, Normal, 60.0, &[20.0], &[30.0], [0.0, 0.0]),
        node(2, Input, 60.0, &[20.0], &[30.0], [400.0, 0.0]),
        node(3, Normal, 60.0, &[20.0], &[30.0], [200.0, 0.0]),
        node(4, Output, 60.0, &[20.0], &[30.0], [-200.0, 0.0]),
    ];
    let edges = vec![edge(1, 0, 2, 0), edge(4, 0, 3, 0), edge(1, 0, 3, 0)];
    let plan = run(&nodes, &edges);
    let cols = columns(&plan);
    assert_eq!(cols[&2], 0);
    assert_eq!(cols[&4], cols.values().copied().max().unwrap());
    assert!(cols[&1] > cols[&2] && cols[&3] > cols[&1] && cols[&4] > cols[&3]);
    assert_no_overlap(&nodes, &plan);
}

#[test]
fn a_constant_sits_just_left_of_its_only_consumer() {
    use ArrangeRole::*;
    let (mut nodes, mut edges) = chain();
    nodes[3].input_ports.push(130.0);
    nodes.push(node(5, Normal, 40.0, &[], &[20.0], [-500.0, 900.0]));
    edges.push(edge(5, 0, 4, 1));
    let plan = run(&nodes, &edges);
    let cols = columns(&plan);
    assert_eq!(nodes[3].role, Output);
    assert_eq!(cols[&4], 3);
    assert_eq!(cols[&5], 2, "the constant feeding only the output");
    assert_no_overlap(&nodes, &plan);
}

#[test]
fn every_edge_runs_from_a_left_column_to_a_right_one() {
    let (nodes, edges) = super::tests_layout::random_dag(40, 70, 3);
    let plan = run(&nodes, &edges);
    let cols = columns(&plan);
    for e in &edges {
        assert!(
            cols[&e.from.0 .0] < cols[&e.to.0 .0],
            "edge {e:?} runs backwards"
        );
    }
    for n in &nodes {
        match n.role {
            ArrangeRole::Input => assert_eq!(cols[&n.id.0], 0),
            ArrangeRole::Output => assert_eq!(cols[&n.id.0], cols.values().copied().max().unwrap()),
            ArrangeRole::Normal => {}
        }
    }
    assert_no_overlap(&nodes, &plan);
}
