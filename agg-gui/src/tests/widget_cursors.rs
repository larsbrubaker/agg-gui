//! Default mouse-cursor conventions for interactive widgets.
//!
//! Every widget that has a conventional cursor (desktop native / egui)
//! must set it from its own `on_event` with no app code: splitters show
//! a resize arrow, text inputs the I-beam, links the pointing hand.
//! These tests drive the real widgets — through [`App`] where hit-testing
//! and mouse capture matter — and read [`crate::current_cursor_icon`]
//! the way the platform shells do after each mouse move. DragValue's
//! cases live beside it in `widgets/drag_value/tests.rs` (they need its
//! private edit-mode entry).

use super::*;
use crate::geometry::{Point, Rect};
use crate::text::Font;
use crate::{current_cursor_icon, reset_cursor_icon, CursorIcon, Event, Hyperlink};
use std::sync::Arc;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

/// Framework-equivalent mouse move for a lone widget: reset, then
/// dispatch (what `App::on_mouse_move` does before hit-testing).
fn hover(w: &mut dyn Widget, x: f64, y: f64) -> CursorIcon {
    reset_cursor_icon();
    w.on_event(&Event::MouseMove {
        pos: Point::new(x, y),
    });
    current_cursor_icon()
}

fn press(w: &mut dyn Widget, x: f64, y: f64) {
    w.on_event(&Event::MouseDown {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

fn release(w: &mut dyn Widget, x: f64, y: f64) {
    w.on_event(&Event::MouseUp {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

/// A 400×200 app whose root is `splitter`. Screen coords are Y-down.
fn splitter_app(splitter: Splitter) -> App {
    let mut app = App::new(Box::new(splitter));
    app.layout(Size::new(400.0, 200.0));
    app
}

/// Hovering a left|right splitter's bar shows the horizontal resize
/// cursor; it persists through a captured drag that leaves the bar, and
/// moving off the bar afterwards restores the arrow.
#[test]
fn splitter_horizontal_sets_resize_cursor_on_hover_and_drag() {
    let mut app = splitter_app(Splitter::new(
        Box::new(SizedBox::new()),
        Box::new(SizedBox::new()),
    ));
    // Divider spans x ≈ 197..203.
    app.on_mouse_move(50.0, 100.0);
    assert_eq!(current_cursor_icon(), CursorIcon::Default, "over a pane");
    app.on_mouse_move(200.0, 100.0);
    assert_eq!(current_cursor_icon(), CursorIcon::ResizeHorizontal);

    app.on_mouse_down(200.0, 100.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(60.0, 100.0);
    assert_eq!(
        current_cursor_icon(),
        CursorIcon::ResizeHorizontal,
        "cursor must stay a resize arrow for the whole drag"
    );
    app.on_mouse_up(60.0, 100.0, MouseButton::Left, Modifiers::default());

    // Divider now sits near x = 60; move well clear of it.
    app.on_mouse_move(300.0, 100.0);
    assert_eq!(current_cursor_icon(), CursorIcon::Default, "moved off");
}

/// Top/bottom splitter: vertical resize cursor, same hover/drag rules.
#[test]
fn splitter_vertical_sets_resize_cursor_on_hover_and_drag() {
    let mut app = splitter_app(Splitter::vertical(
        Box::new(SizedBox::new()),
        Box::new(SizedBox::new()),
    ));
    // Divider spans Y-up 97..103, i.e. screen y ≈ 97..103 too (h = 200).
    app.on_mouse_move(200.0, 100.0);
    assert_eq!(current_cursor_icon(), CursorIcon::ResizeVertical);

    app.on_mouse_down(200.0, 100.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(200.0, 20.0);
    assert_eq!(current_cursor_icon(), CursorIcon::ResizeVertical, "drag");
    app.on_mouse_up(200.0, 20.0, MouseButton::Left, Modifiers::default());

    app.on_mouse_move(200.0, 150.0);
    assert_eq!(current_cursor_icon(), CursorIcon::Default, "moved off");
}

/// A text field shows the I-beam while hovered, keeps it during a
/// selection drag that leaves the field, and resets once off it.
#[test]
fn text_field_sets_text_cursor_on_hover_and_selection_drag() {
    let mut field = TextField::new(font()).with_font_size(14.0);
    let size = field.layout(Size::new(200.0, 40.0));
    field.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    let mid_y = size.height * 0.5;

    assert_eq!(hover(&mut field, 20.0, mid_y), CursorIcon::Text);
    assert_eq!(hover(&mut field, -1.0, -1.0), CursorIcon::Default);

    field.on_event(&Event::FocusGained);
    press(&mut field, 20.0, mid_y);
    assert_eq!(
        hover(&mut field, 500.0, mid_y),
        CursorIcon::Text,
        "selection drag outside the field keeps the I-beam"
    );
    release(&mut field, 500.0, mid_y);
    assert_eq!(hover(&mut field, 500.0, mid_y), CursorIcon::Default);
}

/// A text field inside an [`App`] (left pane of a splitter): hover gives
/// the I-beam, the empty right pane gives the arrow.
#[test]
fn text_field_text_cursor_through_app() {
    let mut app = splitter_app(Splitter::new(
        Box::new(TextField::new(font()).with_font_size(14.0)),
        Box::new(SizedBox::new()),
    ));
    app.on_mouse_move(50.0, 100.0);
    assert_eq!(current_cursor_icon(), CursorIcon::Text);
    app.on_mouse_move(300.0, 100.0);
    assert_eq!(current_cursor_icon(), CursorIcon::Default);
}

/// Hyperlinks show the pointing hand while hovered, the arrow otherwise.
#[test]
fn hyperlink_sets_pointing_hand_on_hover() {
    let mut link = Hyperlink::new("egui", font());
    let size = link.layout(Size::new(300.0, 40.0));
    link.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));

    assert_eq!(
        hover(&mut link, size.width * 0.5, size.height * 0.5),
        CursorIcon::PointingHand
    );
    assert_eq!(hover(&mut link, -1.0, -1.0), CursorIcon::Default);
}

/// A multi-line text area keeps the I-beam during a selection drag that
/// leaves its bounds (it already showed it on hover).
#[test]
fn text_area_keeps_text_cursor_during_selection_drag() {
    let mut area = crate::TextArea::new(font()).with_text("one\ntwo\nthree");
    area.layout(Size::new(200.0, 120.0));
    area.set_bounds(Rect::new(0.0, 0.0, 200.0, 120.0));

    assert_eq!(hover(&mut area, 40.0, 100.0), CursorIcon::Text);
    area.on_event(&Event::FocusGained);
    press(&mut area, 40.0, 100.0);
    assert_eq!(
        hover(&mut area, 500.0, 100.0),
        CursorIcon::Text,
        "selection drag outside the area keeps the I-beam"
    );
    release(&mut area, 500.0, 100.0);
    assert_eq!(hover(&mut area, 500.0, 100.0), CursorIcon::Default);
}

/// Releasing a splitter drag away from the bar must restore the arrow
/// immediately — the shells re-apply [`current_cursor_icon`] after a
/// release, so `App::on_mouse_up` has to re-resolve the hover cursor at
/// the release point rather than leave the drag's resize arrow latched
/// until the next mouse move.
#[test]
fn splitter_release_off_bar_restores_default_cursor_without_move() {
    let mut app = splitter_app(Splitter::new(
        Box::new(SizedBox::new()),
        Box::new(SizedBox::new()),
    ));
    app.on_mouse_move(200.0, 100.0);
    app.on_mouse_down(200.0, 100.0, MouseButton::Left, Modifiers::default());
    // Drag only partway; the pointer outruns the bar (bar clamps / lags).
    app.on_mouse_move(60.0, 100.0);
    assert_eq!(current_cursor_icon(), CursorIcon::ResizeHorizontal);
    // Release far from where the bar now sits, with no further move.
    app.on_mouse_up(300.0, 100.0, MouseButton::Left, Modifiers::default());
    assert_eq!(
        current_cursor_icon(),
        CursorIcon::Default,
        "release off the bar must drop the resize arrow without a move"
    );
}

/// Releasing a drag while still over the bar keeps the resize arrow.
#[test]
fn splitter_release_on_bar_keeps_resize_cursor() {
    let mut app = splitter_app(Splitter::new(
        Box::new(SizedBox::new()),
        Box::new(SizedBox::new()),
    ));
    app.on_mouse_move(200.0, 100.0);
    app.on_mouse_down(200.0, 100.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(200.0, 100.0, MouseButton::Left, Modifiers::default());
    assert_eq!(current_cursor_icon(), CursorIcon::ResizeHorizontal);
}
