//! Rust-only tests of `PointerReach` (agg-gui-automation
//! `src/pointer_reach.rs`, the port of agg-sharp
//! `GuiAutomation/PointerReach.cs`): a press reaches a widget when no
//! ancestor refuses it at the widget's centre, and a lookup prefers widgets
//! a press can reach without failing when none can. Its use by
//! `get_widget_by_name` is covered in `runner_named_tests.rs`.

mod support;

use agg_gui::{Point, Rect, Widget};
use agg_gui_automation::pointer_reach::{can_reach, prefer_reachable};
use agg_gui_automation::{NamedHit, ProbeWidget, WidgetHandle};
use support::Switchable;

fn probe(name: &str, x: f64, y: f64, w: f64, h: f64) -> ProbeWidget {
    ProbeWidget::new(name).with_bounds(Rect::new(x, y, w, h))
}

fn hit(root: &dyn Widget, path: &[usize]) -> NamedHit {
    NamedHit {
        handle: WidgetHandle::new(root, path).expect("widget at path"),
        offset_hint: Point::new(0.0, 0.0),
    }
}

/// A 300 × 200 root holding a plain container with a child (path [0, 0]),
/// a press-through container with a child ([1, 0]), and a container whose
/// child sits outside it ([2, 0]).
fn tree() -> ProbeWidget {
    probe("root", 0.0, 0.0, 300.0, 200.0)
        .with_child(Box::new(
            probe("plain", 0.0, 0.0, 100.0, 100.0).with_child(Box::new(probe(
                "reachable",
                10.0,
                10.0,
                20.0,
                20.0,
            ))),
        ))
        .with_child(Box::new(
            Switchable::new("fading", Rect::new(100.0, 0.0, 100.0, 100.0))
                .passing_presses_through()
                .with_child(Box::new(probe("behind glass", 10.0, 10.0, 20.0, 20.0))),
        ))
        .with_child(Box::new(
            probe("small", 200.0, 0.0, 50.0, 50.0)
                .with_child(Box::new(probe("outside", 60.0, 60.0, 20.0, 20.0))),
        ))
}

#[test]
fn rust_only_a_press_reaches_a_widget_every_ancestor_takes_it_through() {
    let root = tree();
    let root: &dyn Widget = &root;

    assert!(can_reach(root, &hit(root, &[0, 0]).handle));
    assert!(
        can_reach(root, &hit(root, &[]).handle),
        "the root has no ancestor to refuse"
    );
}

#[test]
fn rust_only_a_press_does_not_reach_through_an_ancestor_that_refuses_it() {
    let root = tree();
    let root: &dyn Widget = &root;

    assert!(
        !can_reach(root, &hit(root, &[1, 0]).handle),
        "the press-through container"
    );
    assert!(
        !can_reach(root, &hit(root, &[2, 0]).handle),
        "the centre is outside the parent"
    );
}

#[test]
fn rust_only_the_widget_itself_is_not_asked() {
    let root = probe("root", 0.0, 0.0, 300.0, 200.0).with_child(Box::new(
        Switchable::new("shaped", Rect::new(10.0, 10.0, 20.0, 20.0)).passing_presses_through(),
    ));
    let root: &dyn Widget = &root;

    assert!(can_reach(root, &hit(root, &[0]).handle));
}

#[test]
fn rust_only_prefer_reachable_keeps_the_reachable_hits_or_all_when_there_are_none() {
    let root = tree();
    let root: &dyn Widget = &root;

    let mixed = vec![hit(root, &[1, 0]), hit(root, &[0, 0]), hit(root, &[2, 0])];
    let kept: Vec<Vec<usize>> = prefer_reachable(root, mixed)
        .iter()
        .map(|h| h.handle.path().to_vec())
        .collect();
    assert_eq!(kept, vec![vec![0, 0]]);

    let none = vec![hit(root, &[1, 0]), hit(root, &[2, 0])];
    let kept: Vec<Vec<usize>> = prefer_reachable(root, none)
        .iter()
        .map(|h| h.handle.path().to_vec())
        .collect();
    assert_eq!(kept, vec![vec![1, 0], vec![2, 0]]);
}
