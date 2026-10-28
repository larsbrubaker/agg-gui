//! A press on a widget inside a hosted card that lies under another card,
//! driven through agg-gui's real `App` (hit test, preview, pointer capture,
//! layout between press and release): the press raises the card, which
//! reorders the hosted layer's children, and the moves and the release must
//! still reach the pressed widget, so a button there works on the first
//! click and a slider there follows the drag.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{App, Button, Modifiers, MouseButton, Slider, Widget};

use super::hosted::HostedNodeBody;
use super::tests_common::TEST_FONT_FOR_PICKER;
use super::tests_hosted::{node_with_sockets, HostedModel};
use super::*;
use crate::draw::TITLE_HEIGHT;
use crate::model::NodeView;

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};

/// Card 1 at canvas top-left `(100, 400)`, card 2 at `(150, 390)` over
/// card 1's right part; `body` builds each card's body from its node id.
fn app_with_cards(mut body: impl FnMut(u64, Arc<Font>) -> Box<dyn Widget> + 'static) -> App {
    let model = Arc::new(Mutex::new(HostedModel::default()));
    model.lock().unwrap().inner.nodes = vec![
        node_with_sockets(1, [100.0, 400.0]),
        node_with_sockets(2, [150.0, 390.0]),
    ];
    let shared: SharedModel = model;
    let font = Arc::new(Font::from_slice(TEST_FONT_FOR_PICKER).unwrap());
    let editor = NodeEditor::new(shared).with_body_factory(move |n: &NodeView| {
        Some(HostedNodeBody::new(body(n.id.0, Arc::clone(&font))))
    });
    let mut app = App::new(Box::new(editor));
    app.layout(VIEW);
    app
}

/// [`app_with_cards`] with a button counting its clicks in each card.
fn app_with_button_cards() -> (App, Rc<Cell<u32>>, Rc<Cell<u32>>) {
    let (clicks1, clicks2) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let counters = [Rc::clone(&clicks1), Rc::clone(&clicks2)];
    let app = app_with_cards(move |id, font| {
        let counter = Rc::clone(&counters[(id - 1) as usize]);
        Box::new(Button::new("Go", font).on_click(move || counter.set(counter.get() + 1)))
    });
    (app, clicks1, clicks2)
}

/// A point on card 1's body, left of card 2, in the screen's Y-down pixels.
fn on_card1_body() -> (f64, f64) {
    (120.0, VIEW.height - (400.0 - TITLE_HEIGHT - 10.0))
}

fn order(app: &App) -> Vec<NodeId> {
    let editor = app.root().as_any().unwrap().downcast_ref::<NodeEditor>();
    editor.unwrap().hosted_card_order().to_vec()
}

#[test]
fn a_button_in_a_lower_card_clicks_on_the_first_press_and_raises_the_card() {
    let (mut app, clicks1, clicks2) = app_with_button_cards();
    assert_eq!(order(&app), [NodeId(1), NodeId(2)]);
    let (x, y) = on_card1_body();
    let none = Modifiers::default();
    app.on_mouse_down(x, y, MouseButton::Left, none);
    // A frame between press and release: the layer adopts the new order.
    app.layout(VIEW);
    assert_eq!(
        order(&app),
        [NodeId(2), NodeId(1)],
        "the press raised card 1"
    );
    app.on_mouse_up(x, y, MouseButton::Left, none);
    assert_eq!(
        clicks1.get(),
        1,
        "card 1's button clicked on the first press"
    );
    assert_eq!(clicks2.get(), 0, "card 2's button was not pressed");
}

#[test]
fn a_slider_in_a_lower_card_follows_the_drag_after_the_card_is_raised() {
    type Changes = Rc<RefCell<Vec<f64>>>;
    let (changes1, changes2): (Changes, Changes) = Default::default();
    let logs = [Rc::clone(&changes1), Rc::clone(&changes2)];
    let mut app = app_with_cards(move |id, font| {
        let log = Rc::clone(&logs[(id - 1) as usize]);
        Box::new(Slider::new(0.0, 0.0, 100.0, font).on_change(move |v| log.borrow_mut().push(v)))
    });
    let (x, y) = on_card1_body();
    let none = Modifiers::default();
    app.on_mouse_down(x, y, MouseButton::Left, none);
    app.layout(VIEW);
    assert_eq!(
        order(&app),
        [NodeId(2), NodeId(1)],
        "the press raised card 1"
    );
    let last = |log: &Changes| log.borrow().last().copied();
    let pressed = last(&changes1).expect("the press set card 1's slider");
    // Drag right, across where card 2 lay, and let go there: every move
    // reaches card 1's slider as it happens.
    app.on_mouse_move(x + 40.0, y);
    let moved = last(&changes1).unwrap();
    assert!(
        moved > pressed,
        "the first move dragged card 1's slider: {pressed} -> {moved}"
    );
    app.on_mouse_move(x + 80.0, y);
    let dragged = last(&changes1).unwrap();
    assert!(
        dragged > moved,
        "the second move dragged it on: {moved} -> {dragged}"
    );
    app.on_mouse_up(x + 80.0, y, MouseButton::Left, none);
    // The release ended the drag: a later move leaves the value alone.
    app.on_mouse_move(x + 100.0, y);
    assert_eq!(last(&changes1), Some(dragged), "the release ended the drag");
    assert!(
        changes2.borrow().is_empty(),
        "card 2's slider was never dragged"
    );
}
