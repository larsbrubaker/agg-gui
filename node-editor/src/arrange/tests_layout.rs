//! Tests for [`crate::arrange`] about the arrangement as a whole: column
//! order without crossings, repeatable and stable results, components
//! stacked apart, graphs it must survive (cycles, bad ports), and a
//! release-mode timing check on a large graph.

use super::tests::{applied, assert_no_overlap, columns, crossing_count, edge, node, run};
use super::*;

/// A connected acyclic graph of `n` nodes and about `m` edges: the first
/// `inputs` nodes are Inputs (all feeding node `inputs`), the last is the
/// Output, every other node is fed by an earlier one, scattered about
/// the canvas by a fixed pseudo-random sequence.
pub(super) fn random_dag(n: u64, m: usize, inputs: u64) -> (Vec<ArrangeNode>, Vec<ArrangeEdge>) {
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move |modulo: u64| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) % modulo.max(1)
    };
    let mut nodes = Vec::new();
    for id in 0..n {
        let role = if id < inputs {
            ArrangeRole::Input
        } else if id == n - 1 {
            ArrangeRole::Output
        } else {
            ArrangeRole::Normal
        };
        let h = 60.0 + next(5) as f64 * 20.0;
        let pos = [next(2000) as f64, next(2000) as f64];
        nodes.push(node(id, role, h, &[30.0, 45.0, 55.0], &[25.0, 50.0], pos));
    }
    let mut edges = Vec::new();
    for a in 0..inputs {
        edges.push(edge(a, 0, inputs, (a % 3) as usize));
    }
    for b in inputs + 1..n {
        edges.push(edge(next(b), next(2) as usize, b, next(3) as usize));
    }
    while edges.len() < m {
        let b = inputs + 1 + next(n - inputs - 1);
        let a = next(b);
        edges.push(edge(a, next(2) as usize, b, next(3) as usize));
    }
    (nodes, edges)
}

#[test]
fn a_tree_draws_without_crossings() {
    use ArrangeRole::*;
    // Eight leaves → four → two → Output, leaves listed in a scrambled
    // vertical order.
    let mut nodes = vec![node(1, Output, 80.0, &[25.0, 55.0], &[], [0.0, 0.0])];
    let mut edges = Vec::new();
    let scramble = [5.0, 1.0, 7.0, 3.0, 0.0, 6.0, 2.0, 4.0];
    for (k, mid) in [10u64, 11].into_iter().enumerate() {
        nodes.push(node(
            mid,
            Normal,
            80.0,
            &[25.0, 55.0],
            &[40.0],
            [0.0, 900.0 * k as f64],
        ));
        edges.push(edge(mid, 0, 1, 1 - k));
        for j in 0..2u64 {
            let low = 20 + 2 * mid + j;
            nodes.push(node(
                low,
                Normal,
                80.0,
                &[25.0, 55.0],
                &[40.0],
                [0.0, -700.0 * j as f64],
            ));
            edges.push(edge(low, 0, mid, j as usize));
            for i in 0..2u64 {
                let leaf = 100 + 2 * low + i;
                let y = scramble[(leaf % 8) as usize] * 100.0;
                nodes.push(node(leaf, Input, 50.0, &[], &[25.0], [0.0, y]));
                edges.push(edge(leaf, 0, low, i as usize));
            }
        }
    }
    let plan = run(&nodes, &edges);
    assert_eq!(crossing_count(&nodes, &edges, &plan), 0);
    assert_no_overlap(&nodes, &plan);
}

#[test]
fn crossed_feeds_are_uncrossed_in_socket_order() {
    use ArrangeRole::*;
    // A sits above B but feeds the Output's lower socket.
    let nodes = vec![
        node(1, Input, 60.0, &[], &[30.0], [0.0, 100.0]),
        node(2, Input, 60.0, &[], &[30.0], [0.0, -100.0]),
        node(3, Output, 120.0, &[30.0, 90.0], &[], [300.0, 0.0]),
    ];
    let edges = vec![edge(1, 0, 3, 1), edge(2, 0, 3, 0)];
    let before: std::collections::BTreeMap<u64, [f64; 2]> =
        nodes.iter().map(|n| (n.id.0, n.position)).collect();
    assert_eq!(crossing_count(&nodes, &edges, &before), 1);
    let plan = run(&nodes, &edges);
    assert_eq!(crossing_count(&nodes, &edges, &plan), 0);
    assert!(plan[&2][1] > plan[&1][1], "the socket-0 feed goes on top");
    assert_no_overlap(&nodes, &plan);
}

#[test]
fn arranging_twice_gives_bit_identical_results() {
    let (nodes, edges) = random_dag(60, 100, 4);
    let a = arrange(&nodes, &edges, &ArrangeOptions::default());
    let b = arrange(&nodes, &edges, &ArrangeOptions::default());
    let bits = |v: &[(NodeId, [f64; 2])]| -> Vec<(u64, u64, u64)> {
        v.iter()
            .map(|(id, p)| (id.0, p[0].to_bits(), p[1].to_bits()))
            .collect()
    };
    assert_eq!(bits(&a), bits(&b));
}

#[test]
fn an_arranged_graph_stays_put() {
    for (nodes, edges) in [
        super::tests::chain(),
        random_dag(60, 100, 4),
        random_dag(25, 30, 2),
    ] {
        let first = run(&nodes, &edges);
        let again = run(&applied(&nodes, &first), &edges);
        for (id, p) in &first {
            assert_eq!(
                (p[0].to_bits(), p[1].to_bits()),
                (again[id][0].to_bits(), again[id][1].to_bits()),
                "node {id} moved on the second arrange: {p:?} -> {:?}",
                again[id]
            );
        }
    }
}

#[test]
fn separate_graphs_stack_top_to_bottom_in_their_current_order() {
    use ArrangeRole::*;
    let nodes = vec![
        // The lower graph today: two nodes.
        node(1, Input, 60.0, &[], &[30.0], [0.0, -500.0]),
        node(2, Output, 90.0, &[40.0], &[], [200.0, -500.0]),
        // The upper graph today, and a lone node in between.
        node(3, Input, 70.0, &[], &[30.0], [900.0, 400.0]),
        node(4, Normal, 50.0, &[20.0], &[], [1200.0, 450.0]),
        node(5, Normal, 40.0, &[], &[], [50.0, 0.0]),
    ];
    let edges = vec![edge(1, 0, 2, 0), edge(3, 0, 4, 0)];
    let plan = run(&nodes, &edges);
    assert_no_overlap(&nodes, &plan);
    let gap = ArrangeOptions::default().component_gap;
    // Upper graph first, its top at the old top (450); the lone node next.
    assert_eq!(plan[&4][1].max(plan[&3][1]), 450.0);
    let upper_bottom = (plan[&3][1] - 70.0).min(plan[&4][1] - 50.0);
    assert_eq!(plan[&5][1], upper_bottom - gap);
    assert_eq!(plan[&1][1].max(plan[&2][1]), plan[&5][1] - 40.0 - gap);
    // Every component starts in the first column.
    assert_eq!([plan[&3][0], plan[&5][0], plan[&1][0]], [0.0, 0.0, 0.0]);
}

#[test]
fn cycles_and_bad_edges_still_arrange() {
    use ArrangeRole::Normal;
    let nodes = vec![
        node(1, Normal, 60.0, &[20.0], &[30.0], [0.0, 0.0]),
        node(2, Normal, 60.0, &[20.0], &[30.0], [200.0, 0.0]),
        node(3, Normal, 60.0, &[20.0], &[30.0], [400.0, 0.0]),
        // A repeated id keeps its first entry only.
        node(3, Normal, 10.0, &[], &[], [9.0, 9.0]),
    ];
    let edges = vec![
        edge(1, 0, 2, 0),
        edge(2, 0, 3, 0),
        edge(3, 0, 1, 0),
        edge(1, 0, 2, 0),
        edge(2, 0, 2, 0),
        edge(1, 7, 3, 9),
        edge(1, 0, 99, 0),
    ];
    let result = arrange(&nodes, &edges, &ArrangeOptions::default());
    assert_eq!(result.len(), 3);
    let plan = run(&nodes, &edges);
    assert_no_overlap(&nodes[..3], &plan);
    assert!(plan.values().all(|p| p[0].is_finite() && p[1].is_finite()));
}

#[test]
fn a_long_noodle_keeps_its_own_lane() {
    use ArrangeRole::*;
    // Input feeds both the start of a three-card chain and the Output
    // directly; the long noodle's lane must not overlap the chain cards.
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
    // The long noodle passes the middle cards above or below, a row gap clear,
    // wherever it is straight across their column.
    let lane_y = plan[&1][1] - 30.0;
    let end_y = plan[&4][1] - 70.0;
    if lane_y == end_y {
        for id in [2, 3] {
            let (top, bottom) = (plan[&id][1], plan[&id][1] - 100.0);
            assert!(
                lane_y > top || lane_y < bottom,
                "the noodle runs through card {id}"
            );
        }
    }
}

/// Run with `cargo test --release -p agg-gui-node-editor arrange_timing -- --ignored --nocapture`.
#[test]
#[ignore]
fn arrange_timing_200_nodes_300_edges() {
    let (nodes, edges) = random_dag(200, 300, 6);
    let options = ArrangeOptions::default();
    let start = web_time::Instant::now();
    let runs = 20;
    for _ in 0..runs {
        std::hint::black_box(arrange(&nodes, &edges, &options));
    }
    let each = start.elapsed().as_secs_f64() * 1000.0 / runs as f64;
    println!(
        "arrange: 200 nodes, {} edges: {each:.3} ms per run",
        edges.len()
    );
    assert!(each < 10.0);
}
