//! Tests for [`crate::arrange`] with Input and Output cards that have no
//! noodles: they join the main graph's first and last columns instead of
//! being stacked apart, so the input always reads on the left and the
//! output on the right.

use std::collections::BTreeMap;

use super::tests::{applied, assert_no_overlap, columns, edge, node, run};
use super::*;

/// Arrange, check the plan has no overlaps and that arranging it again
/// changes nothing; the plan.
fn settled(nodes: &[ArrangeNode], edges: &[ArrangeEdge]) -> BTreeMap<u64, [f64; 2]> {
    let plan = run(nodes, edges);
    assert_no_overlap(nodes, &plan);
    assert_eq!(
        plan,
        run(&applied(nodes, &plan), edges),
        "a second arrange moved nodes"
    );
    plan
}

fn others(plan: &BTreeMap<u64, [f64; 2]>, id: u64) -> impl Iterator<Item = [f64; 2]> + '_ {
    plan.iter().filter(move |(&k, _)| k != id).map(|(_, &p)| p)
}

fn card(id: u64, role: ArrangeRole, pos: [f64; 2]) -> ArrangeNode {
    node(id, role, 60.0, &[20.0, 40.0], &[30.0], pos)
}

/// Normal cards 2 → 3 → 4 in a row.
fn middle_chain() -> (Vec<ArrangeNode>, Vec<ArrangeEdge>) {
    use ArrangeRole::Normal;
    let nodes = vec![
        card(2, Normal, [0.0, 0.0]),
        card(3, Normal, [200.0, 0.0]),
        card(4, Normal, [400.0, 0.0]),
    ];
    (nodes, vec![edge(2, 0, 3, 0), edge(3, 0, 4, 0)])
}

#[test]
fn an_unwired_input_goes_left_of_the_graph() {
    let (mut nodes, mut edges) = middle_chain();
    nodes.push(card(5, ArrangeRole::Output, [600.0, 0.0]));
    edges.push(edge(4, 0, 5, 0));
    // Where an earlier arrange stacked it: under the graph, at its left.
    nodes.push(card(1, ArrangeRole::Input, [0.0, -200.0]));
    let plan = settled(&nodes, &edges);
    assert!(others(&plan, 1).all(|p| plan[&1][0] < p[0]), "{plan:?}");
    assert_eq!(columns(&plan)[&1], 0);
    // At the top of its column: the graph's top edge.
    assert!(others(&plan, 1).all(|p| plan[&1][1] >= p[1]), "{plan:?}");
}

#[test]
fn an_unwired_output_goes_right_of_the_graph() {
    let (mut nodes, mut edges) = middle_chain();
    nodes.push(card(1, ArrangeRole::Input, [-200.0, 0.0]));
    edges.push(edge(1, 0, 2, 0));
    nodes.push(card(9, ArrangeRole::Output, [-500.0, 900.0]));
    let plan = settled(&nodes, &edges);
    assert!(others(&plan, 9).all(|p| plan[&9][0] > p[0]), "{plan:?}");
    assert!(others(&plan, 9).all(|p| plan[&9][1] >= p[1]), "{plan:?}");
}

#[test]
fn unwired_input_and_output_frame_a_middle_chain() {
    let (mut nodes, edges) = middle_chain();
    nodes.push(card(1, ArrangeRole::Input, [300.0, 400.0]));
    nodes.push(card(9, ArrangeRole::Output, [-300.0, -400.0]));
    let plan = settled(&nodes, &edges);
    assert!(others(&plan, 1).all(|p| plan[&1][0] < p[0]), "{plan:?}");
    assert!(others(&plan, 9).all(|p| plan[&9][0] > p[0]), "{plan:?}");
    let cols = columns(&plan);
    assert_eq!(
        [cols[&1], cols[&2], cols[&3], cols[&4], cols[&9]],
        [0, 1, 2, 3, 4]
    );
}

#[test]
fn an_unwired_input_and_output_alone_sit_in_one_row() {
    let nodes = vec![
        card(1, ArrangeRole::Input, [500.0, 0.0]),
        card(2, ArrangeRole::Output, [0.0, 300.0]),
    ];
    let plan = settled(&nodes, &[]);
    assert!(plan[&1][0] < plan[&2][0], "{plan:?}");
    assert_eq!(plan[&1][1], plan[&2][1]);
    // Anchored at the old top-left corner.
    assert_eq!([plan[&1][0], plan[&1][1]], [0.0, 300.0]);
}

#[test]
fn an_unwired_input_stacks_with_the_wired_one() {
    use ArrangeRole::*;
    let nodes = vec![
        card(1, Input, [0.0, 0.0]),
        card(2, Normal, [200.0, 0.0]),
        card(3, Output, [400.0, 0.0]),
        card(4, Input, [0.0, -300.0]),
    ];
    let edges = vec![edge(1, 0, 2, 0), edge(2, 0, 3, 0)];
    let plan = settled(&nodes, &edges);
    let cols = columns(&plan);
    assert_eq!([cols[&1], cols[&4], cols[&2], cols[&3]], [0, 0, 1, 2]);
    assert!(
        plan[&4][1] > plan[&1][1],
        "the unwired input on top: {plan:?}"
    );
}

#[test]
fn unwired_roles_join_the_largest_graph_and_others_stay_stacked() {
    use ArrangeRole::*;
    let (mut nodes, mut edges) = middle_chain();
    // A smaller separate graph above, and the unwired roles.
    nodes.push(card(20, Normal, [0.0, 800.0]));
    nodes.push(card(21, Normal, [200.0, 800.0]));
    edges.push(edge(20, 0, 21, 0));
    nodes.push(card(1, Input, [0.0, -400.0]));
    nodes.push(card(9, Output, [0.0, -600.0]));
    let plan = settled(&nodes, &edges);
    let main_x: Vec<f64> = [2, 3, 4].iter().map(|id| plan[id][0]).collect();
    assert!(main_x.iter().all(|&x| plan[&1][0] < x && x < plan[&9][0]));
    // The roles share the main graph's rows, below the smaller graph.
    for id in [1, 9] {
        assert!(plan[&id][1] < plan[&20][1] - 60.0, "{plan:?}");
        assert!((plan[&id][1] - plan[&2][1]).abs() < 200.0, "{plan:?}");
    }
}
