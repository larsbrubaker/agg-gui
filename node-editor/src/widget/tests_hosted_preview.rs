//! Tests for the editor's `Widget::preview_event` in hosted-card mode
//! (`hosted_events.rs`): a press on a real widget inside a card body —
//! here an agg-gui `Slider` — selects and raises the card while the slider
//! still receives the press, through the framework's own hit-test and
//! dispatch.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::text::Font;
use agg_gui::{Modifiers, MouseButton, Point, Slider};

use super::hosted::HostedNodeBody;
use super::tests_common::TEST_FONT_FOR_PICKER;
use super::tests_hosted::{card_by_id, dispatch, node_with_sockets, press, release, HostedModel};
use super::*;
use crate::draw::TITLE_HEIGHT;
use crate::model::NodeView;

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};

/// Two hosted nodes, each a card whose body is a 0..100 `Slider` starting
/// at 0. `last_value` records the latest value any slider reported.
fn slider_editor() -> (NodeEditor, Arc<Mutex<HostedModel>>, Rc<Cell<f64>>) {
    let model = Arc::new(Mutex::new(HostedModel::default()));
    model.lock().unwrap().inner.nodes = vec![
        node_with_sockets(1, [100.0, 400.0]),
        node_with_sockets(2, [400.0, 400.0]),
    ];
    let shared: SharedModel = model.clone();
    let font = Arc::new(Font::from_slice(TEST_FONT_FOR_PICKER).unwrap());
    let last_value = Rc::new(Cell::new(0.0));
    let lv = last_value.clone();
    let mut editor = NodeEditor::new(shared).with_body_factory(move |_n: &NodeView| {
        let lv = lv.clone();
        let slider = Slider::new(0.0, 0.0, 100.0, Arc::clone(&font)).on_change(move |v| lv.set(v));
        Some(HostedNodeBody::new(Box::new(slider)))
    });
    editor.layout(VIEW);
    (editor, model, last_value)
}

/// Editor-local point over card `id`'s body: 30 % across, mid-height.
fn on_body(editor: &NodeEditor, id: NodeId) -> Point {
    let card = card_by_id(editor, id);
    let (c, b) = (card.bounds(), card.children()[0].bounds());
    Point::new(c.x + b.width * 0.3, c.y + b.y + b.height * 0.5)
}

#[test]
fn a_press_on_a_body_slider_selects_and_raises_the_card_and_moves_the_slider() {
    let (mut editor, _model, last_value) = slider_editor();
    assert_eq!(editor.hosted_card_order(), &[NodeId(1), NodeId(2)]);
    let p = on_body(&editor, NodeId(1));
    let r = dispatch(&mut editor, press(p), p);
    assert!(r.is_consumed(), "the slider consumed the press");
    assert!(
        last_value.get() > 0.0,
        "the slider got the press and moved to it"
    );
    assert!(editor.selected_ids().contains(&NodeId(1)));
    editor.layout(VIEW);
    assert_eq!(editor.hosted_card_order(), &[NodeId(2), NodeId(1)]);
    assert!(card_by_id(&editor, NodeId(1)).chrome.selected);
    // The press never reached the editor's handlers: no node drag started.
    assert!(matches!(editor.interaction, CanvasState::Idle));
    dispatch(&mut editor, release(p), p);
}

#[test]
fn a_body_press_replaces_the_selection_and_shift_adds_to_it() {
    let (mut editor, model, _) = slider_editor();
    let p1 = on_body(&editor, NodeId(1));
    dispatch(&mut editor, press(p1), p1);
    dispatch(&mut editor, release(p1), p1);
    assert_eq!(model.lock().unwrap().primary_selection(), Some(NodeId(1)));

    let p2 = on_body(&editor, NodeId(2));
    dispatch(&mut editor, press(p2), p2);
    dispatch(&mut editor, release(p2), p2);
    assert_eq!(editor.selected_ids().len(), 1);
    assert!(editor.selected_ids().contains(&NodeId(2)));
    assert_eq!(model.lock().unwrap().primary_selection(), Some(NodeId(2)));

    let shift = Event::MouseDown {
        pos: p1,
        button: MouseButton::Left,
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    };
    dispatch(&mut editor, shift, p1);
    let ids = editor.selected_ids();
    assert!(ids.contains(&NodeId(1)) && ids.contains(&NodeId(2)));
}

#[test]
fn a_press_on_empty_canvas_or_a_socket_is_not_a_card_preview() {
    let (mut editor, _model, _) = slider_editor();
    let empty = Point::new(700.0, 100.0);
    dispatch(&mut editor, press(empty), empty);
    assert!(editor.selected_ids().is_empty());
    // The Result output socket sits on card 1's right edge, half a row
    // under the title: it starts a noodle and selects nothing.
    let socket = Point::new(
        100.0 + crate::draw::NODE_WIDTH,
        400.0 - TITLE_HEIGHT - crate::draw::ROW_HEIGHT * 0.5,
    );
    dispatch(&mut editor, press(socket), socket);
    assert!(editor.selected_ids().is_empty());
    assert!(matches!(
        editor.interaction,
        CanvasState::DrawingConnection { .. }
    ));
}
