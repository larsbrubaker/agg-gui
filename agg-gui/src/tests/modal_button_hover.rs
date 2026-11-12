//! A [`Button`] inside a showing [`ModalSheet`] shows its hover fill while
//! the pointer is over it and its resting fill once the pointer leaves, every
//! time: moving to the sheet's text, straight to the next button, out of the
//! sheet, out of the window, and after a press slid off it. The modal pointer
//! route (`widget/app/pointer.rs`) sends the widget the pointer left the
//! same `(-1, -1)` hover-clear move the ordinary route does
//! (`widget/app/hover_chain.rs`), and a Button also lets go of its hover on
//! `MouseLeave`. The keyboard focus ring is for focus the keyboard gave: a
//! press that ends off the button leaves no ring, and Tab still draws it.

use super::*;
use crate::widgets::{ButtonTheme, Label, ModalSheet};
use crate::{Event, Rect, Stack};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::text::Font;
use crate::widget::find_widget_screen_rect;

const VIEW: Size = Size {
    width: 400.0,
    height: 400.0,
};
const REST: [u8; 3] = [200, 40, 40];
const HOVER: [u8; 3] = [40, 200, 40];
const PRESSED: [u8; 3] = [40, 40, 200];
const RING: [u8; 3] = [255, 0, 255];
const BUTTONS: [&str; 2] = ["First", "Second"];

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

fn rgb(c: [u8; 3]) -> Color {
    Color::rgb(
        f32::from(c[0]) / 255.0,
        f32::from(c[1]) / 255.0,
        f32::from(c[2]) / 255.0,
    )
}

/// Opaque, distinct colours for each state, so a pixel names the state.
fn theme() -> ButtonTheme {
    ButtonTheme {
        background: rgb(REST),
        background_hovered: rgb(HOVER),
        background_pressed: rgb(PRESSED),
        focus_ring_color: rgb(RING),
        focus_ring_width: 3.0,
        border_radius: 0.0,
        ..ButtonTheme::default()
    }
}

fn button(name: &str) -> Button {
    Button::new(name, font())
        .with_theme(theme())
        .with_min_size(Size::new(90.0, 30.0))
        .with_name(name)
}

/// A dialog: a line of text over two buttons, in a sheet over the window.
fn dialog() -> App {
    let content = FlexColumn::new()
        .add(Box::new(
            Label::new("Save your work?", font()).with_name("Message"),
        ))
        .add(Box::new(
            FlexRow::new()
                .add(Box::new(button(BUTTONS[0])))
                .add(Box::new(button(BUTTONS[1]))),
        ));
    let sheet = ModalSheet::new(Rc::new(Cell::new(true)), Box::new(content))
        .with_panel_size(Size::new(300.0, 200.0));
    let stack = Stack::new()
        .add(Box::new(SizedBox::new()))
        .add(Box::new(sheet));
    let mut app = App::new(Box::new(stack));
    app.layout(VIEW);
    app
}

fn rect(app: &App, id: &str) -> Rect {
    find_widget_screen_rect(app.root(), id).unwrap_or_else(|| panic!("{id} is laid out"))
}

/// The Y-down screen point a press lands at on the widget named `id`: halfway
/// up, 5 in from its left, on the button itself rather than its label, so
/// the press focuses the button.
fn press_point(app: &App, id: &str) -> (f64, f64) {
    let r = rect(app, id);
    (r.x + 5.0, VIEW.height - (r.y + r.height / 2.0))
}

/// Keyboard focus to the next widget. A sheet keeps a plain Tab from its
/// content's focus traversal, so this is the Ctrl+Tab every scope takes.
fn tab(app: &mut App) {
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    app.on_key_down(Key::Tab, ctrl);
}

/// The Y-down screen point at the centre of the widget named `id`.
fn centre(app: &App, id: &str) -> (f64, f64) {
    let r = rect(app, id);
    (r.x + r.width / 2.0, VIEW.height - (r.y + r.height / 2.0))
}

fn move_to(app: &mut App, (x, y): (f64, f64)) {
    app.on_mouse_move(x, y);
}

fn paint(app: &mut App) -> Framebuffer {
    app.layout(VIEW);
    let mut fb = Framebuffer::new(VIEW.width as u32, VIEW.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
    drop(ctx);
    fb
}

/// The colour at Y-up `(x, y)`.
fn pixel(fb: &Framebuffer, x: f64, y: f64) -> [u8; 3] {
    let i = ((y as usize) * fb.width() as usize + x as usize) * 4;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2]]
}

/// A button's fill: halfway up, 8 in from its left, clear of the ring and
/// the label.
fn fill(app: &App, fb: &Framebuffer, id: &str) -> [u8; 3] {
    let r = rect(app, id);
    pixel(fb, r.x + 8.0, r.y + r.height / 2.0)
}

/// A button's edge: halfway up, one in from its left, where the ring is.
fn edge(app: &App, fb: &Framebuffer, id: &str) -> [u8; 3] {
    let r = rect(app, id);
    pixel(fb, r.x + 1.0, r.y + r.height / 2.0)
}

/// `hovered` shows the hover fill and every other button the resting one.
fn assert_fills(app: &mut App, hovered: Option<&str>, when: &str) {
    let fb = paint(app);
    for id in BUTTONS {
        let want = if Some(id) == hovered { HOVER } else { REST };
        assert_eq!(fill(app, &fb, id), want, "{when}: {id}'s fill");
    }
}

#[test]
fn a_button_in_a_sheet_rests_again_each_time_the_pointer_moves_off_it() {
    let mut app = dialog();
    let text = centre(&app, "Message");
    move_to(&mut app, text);
    assert_fills(&mut app, None, "over the text");
    for round in 1..=3 {
        for id in BUTTONS {
            let over = centre(&app, id);
            move_to(&mut app, over);
            assert_fills(&mut app, Some(id), &format!("round {round}, over {id}"));
            move_to(&mut app, text);
            assert_fills(&mut app, None, &format!("round {round}, off {id}"));
        }
    }
}

#[test]
fn moving_straight_to_the_next_button_in_a_sheet_moves_the_hover() {
    let mut app = dialog();
    for round in 1..=2 {
        for id in BUTTONS.iter().chain(BUTTONS.iter().rev()) {
            let over = centre(&app, id);
            move_to(&mut app, over);
            assert_fills(&mut app, Some(id), &format!("round {round}, onto {id}"));
        }
    }
}

#[test]
fn leaving_the_sheet_or_the_window_from_a_button_puts_it_at_rest() {
    let mut app = dialog();
    for id in BUTTONS {
        let over = centre(&app, id);
        move_to(&mut app, over);
        assert_fills(&mut app, Some(id), &format!("over {id}"));
        // The dimmed window around the sheet.
        move_to(&mut app, (4.0, 4.0));
        assert_fills(&mut app, None, &format!("out of the sheet from {id}"));
        let over = centre(&app, id);
        move_to(&mut app, over);
        app.on_mouse_leave();
        assert_fills(&mut app, None, &format!("out of the window from {id}"));
    }
}

#[test]
fn a_press_slid_off_a_button_in_a_sheet_leaves_it_at_rest_and_unringed() {
    let mut app = dialog();
    let text = centre(&app, "Message");
    for id in BUTTONS {
        let (x, y) = press_point(&app, id);
        move_to(&mut app, (x, y));
        app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
        let fb = paint(&mut app);
        assert_eq!(fill(&app, &fb, id), PRESSED, "{id} pressed");
        assert_ne!(edge(&app, &fb, id), RING, "a press draws no ring on {id}");
        move_to(&mut app, text);
        app.on_mouse_up(text.0, text.1, MouseButton::Left, Modifiers::default());
        let fb = paint(&mut app);
        assert_eq!(fill(&app, &fb, id), REST, "{id} after the slide off");
        assert_eq!(
            edge(&app, &fb, id),
            REST,
            "no ring on {id} after the slide off"
        );
    }
}

#[test]
fn tab_focus_still_rings_a_button_in_a_sheet() {
    let mut app = dialog();
    tab(&mut app);
    let fb = paint(&mut app);
    assert_eq!(app.focused_widget_type_name(), Some("Button"));
    let ringed: Vec<&str> = BUTTONS
        .into_iter()
        .filter(|id| edge(&app, &fb, id) == RING)
        .collect();
    assert_eq!(ringed.len(), 1, "one button shows the ring: {ringed:?}");
    // The ring is an outline: the fill stays at rest.
    assert_eq!(fill(&app, &fb, ringed[0]), REST);
}

#[test]
fn a_press_on_a_tab_focused_button_hides_its_ring_and_the_next_tab_shows_one() {
    let mut app = dialog();
    tab(&mut app);
    let fb = paint(&mut app);
    let first = BUTTONS
        .into_iter()
        .find(|id| edge(&app, &fb, id) == RING)
        .expect("Tab rings a button");
    let text = centre(&app, "Message");
    let (x, y) = press_point(&app, first);
    move_to(&mut app, (x, y));
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    move_to(&mut app, text);
    app.on_mouse_up(text.0, text.1, MouseButton::Left, Modifiers::default());
    let fb = paint(&mut app);
    assert_eq!(
        edge(&app, &fb, first),
        REST,
        "the pointer took the ring away"
    );
    tab(&mut app);
    let fb = paint(&mut app);
    let ringed = BUTTONS
        .into_iter()
        .filter(|id| edge(&app, &fb, id) == RING)
        .count();
    assert_eq!(ringed, 1, "Tab shows the ring again");
}

/// A Button on its own, painted at its origin.
fn paint_button(button: &mut Button) -> [u8; 3] {
    let mut fb = Framebuffer::new(100, 30);
    let mut ctx = GfxCtx::new(&mut fb);
    button.paint(&mut ctx);
    drop(ctx);
    pixel(&fb, 8.0, 15.0)
}

#[test]
fn a_button_lets_go_of_its_hover_and_press_when_the_pointer_leaves_it() {
    let mut b = button("Alone");
    b.layout(Size::new(100.0, 30.0));
    b.set_bounds(Rect::new(0.0, 0.0, 100.0, 30.0));
    b.on_event(&Event::MouseMove {
        pos: crate::Point::new(20.0, 15.0),
    });
    assert_eq!(paint_button(&mut b), HOVER);
    b.on_event(&Event::MouseLeave);
    assert_eq!(paint_button(&mut b), REST, "MouseLeave clears the hover");
    b.on_event(&Event::MouseDown {
        pos: crate::Point::new(20.0, 15.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(paint_button(&mut b), PRESSED);
    b.on_event(&Event::MouseLeave);
    assert_eq!(paint_button(&mut b), REST, "MouseLeave clears the press");
}

/// A fresh button where the focused one stands, as an app rebuilding that
/// part of the tree (a page change, a list refresh) puts one.
fn rebuild_focused_button(app: &mut App, name: &str) {
    let path = app
        .focused_path()
        .map(|p| p.to_vec())
        .expect("a button has focus");
    let (last, parent) = path.split_last().expect("the button is below the root");
    let mut widget: &mut dyn Widget = app.root_mut();
    for &i in parent {
        widget = widget.children_mut()[i].as_mut();
    }
    widget.children_mut()[*last] = Box::new(button(name));
    app.layout(VIEW);
}

#[test]
fn a_button_rebuilt_after_a_click_gets_no_ring() {
    let mut app = dialog();
    let (x, y) = press_point(&app, BUTTONS[0]);
    move_to(&mut app, (x, y));
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
    let fb = paint(&mut app);
    assert_ne!(edge(&app, &fb, BUTTONS[0]), RING, "no ring after the click");
    rebuild_focused_button(&mut app, BUTTONS[0]);
    move_to(&mut app, (x, y));
    let fb = paint(&mut app);
    assert_eq!(app.focused_widget_type_name(), Some("Button"));
    assert_ne!(
        edge(&app, &fb, BUTTONS[0]),
        RING,
        "the rebuilt button takes the focus, not a ring"
    );
}

#[test]
fn a_button_rebuilt_after_tab_keeps_the_ring() {
    let mut app = dialog();
    tab(&mut app);
    let fb = paint(&mut app);
    let ringed = BUTTONS
        .into_iter()
        .find(|id| edge(&app, &fb, id) == RING)
        .expect("Tab rings a button");
    rebuild_focused_button(&mut app, ringed);
    let fb = paint(&mut app);
    assert_eq!(
        edge(&app, &fb, ringed),
        RING,
        "the rebuilt button is ringed"
    );
}
