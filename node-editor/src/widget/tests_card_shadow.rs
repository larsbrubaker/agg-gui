//! Tests for `NodeEditor::with_card_shadow` (`presentation.rs`): the drop
//! shadow a hosted card paints round itself (`hosted_card.rs`), agg-gui's
//! window shadow by default, or the host's (MatterCAD's node card: a 3.5
//! unit blur 1.5 units down).

use agg_gui::{Color, Size};

use super::hosted::HostedNodeBody;
use super::tests_common::{mk_node, Memory};
use super::tests_socket_at::EmptyBody;
use super::*;
use crate::model::NodeView;
use crate::test_recorder::{Op, Recorder};

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};

/// The outermost shadow fill a hosted card paints: its rect and colour.
fn outer_shadow(shadow: Option<CardShadow>) -> ([f64; 2], [f64; 2], Color) {
    let m = Memory {
        nodes: vec![mk_node(1, "Box", [100.0, 400.0])],
        ..Memory::default()
    };
    let shared: SharedModel = Arc::new(Mutex::new(m));
    let mut editor = NodeEditor::new(shared).with_body_factory(|_n: &NodeView| {
        Some(HostedNodeBody::new(Box::new(EmptyBody::default())))
    });
    if let Some(shadow) = shadow {
        editor = editor.with_card_shadow(shadow);
    }
    editor.set_bounds(Rect::new(0.0, 0.0, VIEW.width, VIEW.height));
    editor.layout(VIEW);
    let card = editor
        .children_mut()
        .last_mut()
        .and_then(|l| l.children_mut().first_mut())
        .and_then(|c| c.as_any_mut())
        .and_then(|a| a.downcast_mut::<HostedCard>())
        .expect("hosted card");
    let mut r = Recorder::default();
    card.paint(&mut r);
    let first = r.shots.first().expect("the shadow paints first");
    assert!(first.fill);
    match first.path.first() {
        Some(Op::RoundedRect(min, size, _)) => (*min, *size, first.color),
        other => panic!("the shadow is a rounded rect, got {other:?}"),
    }
}

#[test]
fn a_hosted_card_paints_agg_guis_window_shadow_by_default() {
    let (min, _, _) = outer_shadow(None);
    // agg-gui's window shadow: a 14 unit blur in 10 steps, 2 right and 6 down.
    let infl = 0.9 * 14.0;
    assert!((min[0] - (2.0 - infl)).abs() < 1e-9, "{min:?}");
    assert!((min[1] - (-6.0 - infl)).abs() < 1e-9, "{min:?}");
}

#[test]
fn a_hosted_card_paints_the_hosts_card_shadow() {
    let color = Color::rgba(0.0, 0.0, 0.0, 0.5);
    let (min, _, painted) = outer_shadow(Some(CardShadow {
        blur: 3.5,
        offset: [0.0, -1.5],
        color: Some(color),
    }));
    let infl = 0.9 * 3.5;
    assert!((min[0] - (0.0 - infl)).abs() < 1e-9, "{min:?}");
    assert!((min[1] - (-1.5 - infl)).abs() < 1e-9, "{min:?}");
    assert_eq!((painted.r, painted.g, painted.b), (0.0, 0.0, 0.0));
}
