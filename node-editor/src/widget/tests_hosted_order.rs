//! Tests for the hosted cards' paint order (`hosted.rs`,
//! `hosted_events.rs`): by default a pressed card is raised; with
//! `with_raise_on_click(false)` the cards keep model order, so a press
//! selects without raising and where two cards overlap the one later in
//! model order takes it.
//! With raise-on-click on, a left or right press and a socket press in
//! the overlap all go to the card painted on top.

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
    editor_with_card2_at([150.0, 390.0], raise_on_click)
}

/// Card 1 at canvas top-left `(100, 400)`, card 2 at `card2`.
fn editor_with_card2_at(card2: [f64; 2], raise_on_click: Option<bool>) -> NodeEditor {
    let model = Arc::new(Mutex::new(HostedModel::default()));
    model.lock().unwrap().inner.nodes = vec![
        node_with_sockets(1, [100.0, 400.0]),
        node_with_sockets(2, card2),
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

fn right_press(p: Point) -> Event {
    Event::MouseDown {
        pos: p,
        button: agg_gui::MouseButton::Right,
        modifiers: agg_gui::Modifiers::default(),
    }
}

/// Card 2's `Width` input, on its left edge: over card 1's body.
fn card2_input(editor: &NodeEditor) -> Point {
    let layouts = editor.snapshot_layouts();
    let card2 = layouts.iter().find(|l| l.node_id == NodeId(2)).unwrap();
    let s = card2
        .sockets()
        .find(|s| s.name == "Width")
        .expect("card 2's input");
    let card1 = layouts.iter().find(|l| l.node_id == NodeId(1)).unwrap();
    assert!(
        card1.body_contains(s.center),
        "the input lies over card 1: {:?} {:?} {:?}",
        s.center,
        card1.top_left,
        card1.size
    );
    Point::new(s.center[0], s.center[1])
}

/// Card 2 a little higher than in [`overlapping_editor`], so its `Width`
/// input (on its left edge, below the title) lies over card 1's body.
fn input_over_card1(raise_on_click: Option<bool>) -> NodeEditor {
    editor_with_card2_at([150.0, 420.0], raise_on_click)
}

/// Card 1 raised over card 2 by the host.
fn card1_raised() -> NodeEditor {
    let mut editor = input_over_card1(None);
    editor.raise_card(NodeId(1));
    editor.layout(VIEW);
    assert_eq!(editor.hosted_card_order(), &[NodeId(2), NodeId(1)]);
    editor
}

#[test]
fn a_right_press_in_the_overlap_picks_the_raised_card() {
    let mut editor = card1_raised();
    dispatch(&mut editor, right_press(overlap()), overlap());
    assert_eq!(selected(&editor), vec![NodeId(1)]);
    assert_eq!(editor.hosted_card_order(), &[NodeId(2), NodeId(1)]);
}

#[test]
fn a_right_press_raises_the_card() {
    let mut editor = overlapping_editor(None);
    dispatch(&mut editor, right_press(card1_only()), card1_only());
    editor.layout(VIEW);
    assert_eq!(selected(&editor), vec![NodeId(1)]);
    assert_eq!(editor.hosted_card_order(), &[NodeId(2), NodeId(1)]);
}

#[test]
fn a_left_press_in_the_overlap_picks_the_raised_card() {
    let mut editor = card1_raised();
    click(&mut editor, overlap());
    assert_eq!(selected(&editor), vec![NodeId(1)]);
}

#[test]
fn a_socket_covered_by_the_raised_card_takes_no_press() {
    // Through the framework: the press previews as a press on card 1.
    let mut editor = card1_raised();
    let p = card2_input(&editor);
    dispatch(&mut editor, press(p), p);
    assert!(
        !matches!(editor.interaction, CanvasState::DrawingConnection { .. }),
        "card 2's input is hidden under card 1"
    );
    assert_eq!(selected(&editor), vec![NodeId(1)]);
    // The canvas's own hit test agrees: card 1's body, not the socket.
    let mut editor = card1_raised();
    editor.on_event(&press(p));
    assert!(matches!(
        editor.interaction,
        CanvasState::DraggingNode { .. }
    ));
    assert_eq!(selected(&editor), vec![NodeId(1)]);
}

#[test]
fn without_raise_on_click_the_socket_on_top_in_model_order_takes_the_press() {
    let mut editor = input_over_card1(Some(false));
    let p = card2_input(&editor);
    // The canvas's own hit test (the socket hangs below card 2, where the
    // framework's widget hit test finds card 1's body widget instead).
    editor.on_event(&press(p));
    assert!(matches!(
        editor.interaction,
        CanvasState::DrawingConnection {
            from_node: NodeId(2),
            ..
        }
    ));
}

#[test]
fn without_raise_on_click_a_right_press_keeps_model_order() {
    let mut editor = overlapping_editor(Some(false));
    dispatch(&mut editor, right_press(card1_only()), card1_only());
    editor.layout(VIEW);
    assert_eq!(editor.hosted_card_order(), &[NodeId(1), NodeId(2)]);
    editor.popup.close();
    dispatch(&mut editor, right_press(overlap()), overlap());
    assert_eq!(selected(&editor), vec![NodeId(2)]);
}

#[test]
fn a_noodle_is_not_drawn_to_a_socket_covered_by_the_raised_card() {
    let editor = card1_raised();
    let p = card2_input(&editor);
    let layouts = editor.snapshot_layouts();
    let model = editor.model.lock().unwrap();
    // A noodle from card 1's output looking for an input near card 2's.
    let fixed = crate::connection::SocketRef {
        node: NodeId(1),
        side: crate::draw::SocketSide::Output,
        socket: "Result".into(),
        socket_type: SocketTypeId(0),
    };
    let near = super::connect::find_target_near(&layouts, &*model, [p.x, p.y], &fixed, None);
    assert!(near.is_none(), "the covered input rings nothing");
}
