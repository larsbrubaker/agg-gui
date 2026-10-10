//! Tests for the hosted cards' paint order (`hosted.rs`,
//! `hosted_events.rs`): by default a pressed card is raised; with
//! `with_raise_on_click(false)` the cards keep model order, as MatterCAD's
//! NodeDesigner keeps its node windows, so a press selects without raising
//! and where two cards overlap the one later in model order takes it.

use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{Point, Slider};

use super::hosted::HostedNodeBody;
use super::tests_common::TEST_FONT_FOR_PICKER;
use super::tests_hosted::{dispatch, node_with_sockets, press, release, HostedModel};
use super::*;
use crate::draw::TITLE_HEIGHT;
use crate::model::NodeView;

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};

/// Two overlapping hosted cards: card 1 at canvas top-left `(100, 400)`,
/// card 2 at `(150, 390)`, so card 2's title bar lies over card 1's body.
fn overlapping_editor(raise_on_click: Option<bool>) -> NodeEditor {
    let model = Arc::new(Mutex::new(HostedModel::default()));
    model.lock().unwrap().inner.nodes = vec![
        node_with_sockets(1, [100.0, 400.0]),
        node_with_sockets(2, [150.0, 390.0]),
    ];
    let shared: SharedModel = model;
    let font = Arc::new(Font::from_slice(TEST_FONT_FOR_PICKER).unwrap());
    let mut editor = NodeEditor::new(shared).with_body_factory(move |_n: &NodeView| {
        let slider = Slider::new(0.0, 0.0, 100.0, Arc::clone(&font));
        Some(HostedNodeBody::new(Box::new(slider)))
    });
    if let Some(raise) = raise_on_click {
        editor = editor.with_raise_on_click(raise);
    }
    editor.layout(VIEW);
    editor
}

/// Press and release at `p` through the framework's dispatch, then lay out.
fn click(editor: &mut NodeEditor, p: Point) {
    dispatch(editor, press(p), p);
    editor.on_event(&release(p));
    editor.layout(VIEW);
}

/// On card 1's title bar, left of card 2.
fn card1_only() -> Point {
    Point::new(120.0, 400.0 - TITLE_HEIGHT * 0.5)
}

/// On card 2's title bar, where it covers card 1's body.
fn overlap() -> Point {
    Point::new(200.0, 390.0 - TITLE_HEIGHT * 0.5)
}

fn selected(editor: &NodeEditor) -> Vec<NodeId> {
    let mut ids: Vec<NodeId> = editor.selected_ids().iter().copied().collect();
    ids.sort_by_key(|id| id.0);
    ids
}

#[test]
fn without_raise_on_click_pressing_a_lower_card_keeps_model_order() {
    let mut editor = overlapping_editor(Some(false));
    assert_eq!(editor.hosted_card_order(), &[NodeId(1), NodeId(2)]);
    click(&mut editor, card1_only());
    assert_eq!(selected(&editor), vec![NodeId(1)]);
    assert_eq!(editor.hosted_card_order(), &[NodeId(1), NodeId(2)]);
    editor.raise_card(NodeId(1));
    assert_eq!(editor.hosted_card_order(), &[NodeId(1), NodeId(2)]);
}

#[test]
fn without_raise_on_click_an_overlap_press_hits_the_card_later_in_model_order() {
    let mut editor = overlapping_editor(Some(false));
    click(&mut editor, card1_only());
    click(&mut editor, overlap());
    assert_eq!(selected(&editor), vec![NodeId(2)]);
    assert_eq!(editor.hosted_card_order(), &[NodeId(1), NodeId(2)]);
}

#[test]
fn by_default_a_pressed_card_is_raised_and_takes_an_overlap_press() {
    let mut editor = overlapping_editor(None);
    click(&mut editor, card1_only());
    assert_eq!(selected(&editor), vec![NodeId(1)]);
    assert_eq!(editor.hosted_card_order(), &[NodeId(2), NodeId(1)]);
    click(&mut editor, overlap());
    assert_eq!(selected(&editor), vec![NodeId(1)]);
    assert_eq!(editor.hosted_card_order(), &[NodeId(2), NodeId(1)]);
}
