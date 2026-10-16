//! The room a centred [`ModalSheet`] leaves its panel
//! (`ModalSheet::centred_room`, `modal_sheet::EDGE_MARGIN`): what a host asks
//! when it sizes something to the panel, such as a remembered dialog size it
//! wants the sheet to draw at exactly. Pins the margin and checks the room
//! against what a real layout draws.

use super::*;
use crate::widgets::modal_sheet::EDGE_MARGIN;
use crate::widgets::ModalSheet;
use crate::{Rect, Stack};
use std::cell::Cell;
use std::rc::Rc;

/// Lays out a centred sheet wanting `size` in a `host` and returns the
/// panel rect it drew.
fn drawn_panel(host: Size, size: Size) -> Rect {
    let sheet =
        ModalSheet::new(Rc::new(Cell::new(true)), Box::new(SizedBox::new())).with_panel_size(size);
    let stack = Stack::new()
        .add(Box::new(SizedBox::new()))
        .add(Box::new(sheet));
    let mut app = App::new(Box::new(stack));
    app.layout(host);
    app.root().children()[1]
        .as_any()
        .and_then(|a| a.downcast_ref::<ModalSheet>())
        .expect("sheet is the stack's second child")
        .panel_rect()
}

/// The margin consumers size against is 24 on every side.
#[test]
fn centred_room_is_the_host_less_the_edge_margin_on_every_side() {
    assert_eq!(EDGE_MARGIN, 24.0);
    assert_eq!(
        ModalSheet::centred_room(Size::new(800.0, 600.0)),
        Size::new(752.0, 552.0)
    );
}

/// A host smaller than the two margins leaves no room, never a negative one.
#[test]
fn centred_room_never_goes_negative() {
    assert_eq!(
        ModalSheet::centred_room(Size::new(40.0, 10.0)),
        Size::new(0.0, 0.0)
    );
}

/// A desired size equal to the room draws at exactly that size; one larger
/// in either direction is clamped to the room, centred with the margin
/// around it.
#[test]
fn a_centred_sheet_draws_within_its_room() {
    let host = Size::new(800.0, 600.0);
    let room = ModalSheet::centred_room(host);
    assert_eq!(
        drawn_panel(host, room),
        Rect::new(EDGE_MARGIN, EDGE_MARGIN, room.width, room.height)
    );
    assert_eq!(
        drawn_panel(host, Size::new(1000.0, 300.0)),
        Rect::new(EDGE_MARGIN, 150.0, room.width, 300.0)
    );
    assert_eq!(
        drawn_panel(host, Size::new(400.0, 900.0)),
        Rect::new(200.0, EDGE_MARGIN, 400.0, room.height)
    );
}
