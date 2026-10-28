//! Tests for [`crate::arrange`] that hold over many graphs: arranging
//! twice changes nothing (cyclic graphs, role-breaking edges, large dense
//! graphs, positions that went through a host's arithmetic), noodle
//! lanes keep clear of cards, bad numbers stay finite, and one pinned
//! fixture catches any change in the arithmetic's order.

use std::collections::BTreeMap;

use super::tests::{applied, assert_no_overlap, columns, crossing_count, edge, node, run};
use super::*;

/// The probe sequence the generators below draw from.
fn rng(seed: &mut u64, modulo: u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*seed >> 33) % modulo.max(1)
}

/// A small graph with random roles and random edges: cycles, self-loops,
/// edges into Inputs and out of Outputs all occur.
fn messy_graph(trial: u64) -> (Vec<ArrangeNode>, Vec<ArrangeEdge>) {
    let mut s = trial.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let n = 2 + rng(&mut s, 12);
    let m = rng(&mut s, 2 * n + 1);
    let roles = [
        ArrangeRole::Input,
        ArrangeRole::Output,
        ArrangeRole::Normal,
        ArrangeRole::Normal,
        ArrangeRole::Normal,
    ];
    let nodes: Vec<ArrangeNode> = (0..n)
        .map(|i| {
            let role = roles[rng(&mut s, 5) as usize];
            let h = rng(&mut s, 4) as f64 * 30.0;
            let w = 50.0 + rng(&mut s, 3) as f64 * 40.0;
            let pos = [rng(&mut s, 800) as f64, rng(&mut s, 800) as f64];
            let ins = [rng(&mut s, 100) as f64, rng(&mut s, 100) as f64];
            let outs = [rng(&mut s, 100) as f64];
            let mut a = node(i, role, h, &ins, &outs, pos);
            a.size[0] = w;
            a
        })
        .collect();
    let edges = (0..m)
        .map(|_| {
            let (a, ap) = (rng(&mut s, n), rng(&mut s, 2) as usize);
            edge(a, ap, rng(&mut s, n), rng(&mut s, 3) as usize)
        })
        .collect();
    (nodes, edges)
}

/// A sparse to dense acyclic graph of 30–230 nodes, all Normal.
fn dense_dag(trial: u64) -> (Vec<ArrangeNode>, Vec<ArrangeEdge>) {
    let mut s = trial.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let n = 30 + rng(&mut s, 200);
    let m = (n as usize) * (1 + rng(&mut s, 3) as usize);
    let nodes: Vec<ArrangeNode> = (0..n)
        .map(|i| {
            let h = 40.0 + rng(&mut s, 4) as f64 * 20.0;
            let pos = [rng(&mut s, 3000) as f64, rng(&mut s, 3000) as f64];
            node(
                i,
                ArrangeRole::Normal,
                h,
                &[10.0, 20.0, 30.0],
                &[15.0, 25.0],
                pos,
            )
        })
        .collect();
    let edges = (0..m)
        .map(|_| {
            let b = 1 + rng(&mut s, n - 1);
            let a = rng(&mut s, b);
            edge(a, rng(&mut s, 2) as usize, b, rng(&mut s, 3) as usize)
        })
        .collect();
    (nodes, edges)
}

fn assert_idempotent(nodes: &[ArrangeNode], edges: &[ArrangeEdge], what: &str) {
    let first = run(nodes, edges);
    let again = run(&applied(nodes, &first), edges);
    assert_eq!(first, again, "{what}: the second arrange moved nodes");
}

#[test]
fn two_cycles_arrange_the_same_twice() {
    use ArrangeRole::Normal;
    let nodes: Vec<ArrangeNode> = (1..=4)
        .map(|i| {
            node(
                i,
                Normal,
                60.0,
                &[20.0, 40.0],
                &[30.0],
                [(i - 1) as f64 * 100.0, 0.0],
            )
        })
        .collect();
    let edges = vec![
        edge(1, 0, 2, 0),
        edge(2, 0, 1, 0),
        edge(3, 0, 4, 0),
        edge(4, 0, 3, 0),
        edge(4, 0, 2, 0),
    ];
    assert_idempotent(&nodes, &edges, "two cycles");
}

#[test]
fn messy_graphs_arrange_the_same_twice_without_overlaps() {
    for trial in 0..1500 {
        let (nodes, edges) = messy_graph(trial);
        let plan = run(&nodes, &edges);
        assert_no_overlap(&nodes, &plan);
        assert_idempotent(&nodes, &edges, &format!("messy graph {trial}"));
    }
}

#[test]
fn dense_graphs_arrange_the_same_twice() {
    // 359 needed more improving rounds than a fixed cap allowed.
    for trial in [359, 0, 1, 2, 3, 4, 5, 6, 7] {
        let (nodes, edges) = dense_dag(trial);
        assert_idempotent(&nodes, &edges, &format!("dense graph {trial}"));
    }
}

#[test]
fn positions_survive_a_hosts_arithmetic() {
    // A host that stores a card as a window rectangle and reads the
    // top-left back can land a rounding error off; model that as the
    // nearest other double, every position, in both directions.
    let (mut nodes, edges) = super::tests_layout::random_dag(60, 100, 4);
    for n in &mut nodes {
        n.size[1] += 0.3;
    }
    let first = run(&nodes, &edges);
    assert!(first
        .values()
        .all(|p| p[0] == p[0].round() && p[1] == p[1].round()));
    for (k, step) in [(0, 1e-9), (1, -1e-9)] {
        let moved: BTreeMap<u64, [f64; 2]> = first
            .iter()
            .map(|(&id, &p)| {
                let q = if (id + k) % 2 == 0 {
                    [p[0].next_up(), p[1].next_down()]
                } else {
                    [p[0].next_down() + step, p[1].next_up() - step]
                };
                (id, q)
            })
            .collect();
        assert_eq!(first, run(&applied(&nodes, &moved), &edges));
    }
}

#[test]
fn bad_numbers_give_finite_separate_cards() {
    use ArrangeRole::Normal;
    let sizes = [
        [f64::NAN, 50.0],
        [f64::INFINITY, f64::NEG_INFINITY],
        [0.0, 0.0],
        [-10.0, -20.0],
        [1e308, 1e308],
        [1e308, 40.0],
    ];
    let mut nodes: Vec<ArrangeNode> = sizes
        .iter()
        .enumerate()
        .map(|(i, &size)| {
            let mut n = node(
                i as u64,
                Normal,
                0.0,
                &[f64::NAN, 1e308],
                &[f64::INFINITY],
                [0.0, 0.0],
            );
            n.size = size;
            n
        })
        .collect();
    nodes[0].position = [f64::NAN, f64::INFINITY];
    nodes[1].position = [-1e308, 1e308];
    nodes[2].position = [-0.0, -0.0];
    let edges = [edge(0, 0, 1, 0), edge(1, 0, 2, 1), edge(3, 0, 4, 0)];
    for e in [&edges[..], &[]] {
        let plan = run(&nodes, e);
        assert_eq!(plan.len(), nodes.len());
        for p in plan.values() {
            assert!(p[0].is_finite() && p[1].is_finite(), "{plan:?}");
            assert!(p[0].to_bits() != (-0.0f64).to_bits() && p[1].to_bits() != (-0.0f64).to_bits());
        }
        let clean: Vec<ArrangeNode> = nodes
            .iter()
            .map(|n| ArrangeNode {
                size: sanitize_card(n).size(),
                ..n.clone()
            })
            .collect();
        assert_no_overlap(&clean, &plan);
    }
}

/// Every noodle lane point is a row gap clear of every card in its column.
fn assert_lanes_clear(nodes: &[ArrangeNode], edges: &[ArrangeEdge]) {
    let opts = ArrangeOptions::default();
    let index: BTreeMap<u64, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id.0, i)).collect();
    let cards: Vec<Card> = nodes.iter().map(sanitize_card).collect();
    let links = resolve_links(&cards, &index, edges);
    let layout = layout_component(&cards, &links, &opts);
    for lane in &layout.lanes {
        for (c, place) in cards.iter().zip(&layout.places) {
            if place[0] == lane[0] {
                let above = lane[1] <= place[1] - opts.row_gap;
                let below = lane[1] >= place[1] + c.h + opts.row_gap;
                assert!(
                    above || below,
                    "lane {lane:?} runs through card {} at {place:?}",
                    c.id
                );
            }
        }
    }
}

#[test]
fn a_long_noodle_keeps_its_own_lane() {
    use ArrangeRole::*;
    // Input feeds both the start of a three-card chain and the Output
    // directly; the long noodle's lane must keep clear of the chain cards.
    let nodes = vec![
        node(1, Input, 60.0, &[], &[30.0], [0.0, 0.0]),
        node(2, Normal, 100.0, &[50.0], &[50.0], [0.0, 0.0]),
        node(3, Normal, 100.0, &[50.0], &[50.0], [0.0, 0.0]),
        node(4, Output, 100.0, &[30.0, 70.0], &[], [0.0, 0.0]),
    ];
    let edges = vec![
        edge(1, 0, 2, 0),
        edge(2, 0, 3, 0),
        edge(3, 0, 4, 0),
        edge(1, 0, 4, 1),
    ];
    let plan = run(&nodes, &edges);
    assert_no_overlap(&nodes, &plan);
    let cols = columns(&plan);
    assert_eq!([cols[&1], cols[&2], cols[&3], cols[&4]], [0, 1, 2, 3]);
    assert_eq!(crossing_count(&nodes, &edges, &plan), 0);
    assert_lanes_clear(&nodes, &edges);
    let (nodes, edges) = super::tests_layout::random_dag(40, 80, 3);
    assert_lanes_clear(&nodes, &edges);
}

#[test]
fn a_pinned_graph_arranges_to_exact_positions() {
    let (nodes, edges) = super::tests::chain();
    let mut nodes = nodes;
    nodes.push(node(
        5,
        ArrangeRole::Normal,
        45.5,
        &[],
        &[12.25],
        [33.3, 77.7],
    ));
    let mut edges = edges;
    edges.push(edge(5, 0, 2, 1));
    let plan = run(&nodes, &edges);
    // Every noodle of the chain is level (1→2 at 245, 2→3 at 185, 3→4 at
    // 200); the constant sits a row gap under the input, its half-unit
    // height rounded away; the top-left stays at (-300, 300).
    let expected: BTreeMap<u64, [f64; 2]> = [
        (1, [-300.0, 285.0]),
        (2, [-140.0, 275.0]),
        (3, [20.0, 235.0]),
        (4, [180.0, 300.0]),
        (5, [-300.0, 195.0]),
    ]
    .into_iter()
    .collect();
    assert_eq!(plan, expected);
}
