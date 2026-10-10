//! `Clickable` — a composite card (labels, a progress bar, ...) that is one
//! click target. A press and release anywhere on the card fires `on_click`
//! once, whichever child is under the pointer; the card tracks hover and
//! press, takes keyboard focus with Tab, and activates on Enter / Space like
//! a `Button`; a disabled card does neither; GUI automation finds it by name.

use super::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::event::Event;
use crate::geometry::{Point, Rect};
use crate::text::Font;
use crate::widget::find_widget_by_id;
use crate::widgets::{Clickable, Label, ProgressBar};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

/// A fit-height card: two labels over a progress bar.
fn card() -> Container {
    Container::new()
        .with_fit_height(true)
        .with_padding(8.0)
        .with_corner_radius(6.0)
        .add(Box::new(Label::new("Disk", font())))
        .add(Box::new(Label::new("120 GB free", font())))
        .add(Box::new(ProgressBar::new(0.4, font())))
}

fn counter(count: &Rc<Cell<u32>>) -> impl FnMut() + 'static {
    let c = Rc::clone(count);
    move || c.set(c.get() + 1)
}

/// A 300 x 300 app whose column holds the card at its top.
fn app_with(clickable: Clickable) -> App {
    let mut col = FlexColumn::new();
    col.push(Box::new(clickable), 0.0);
    let mut app = App::new(Box::new(col));
    app.layout(Size::new(300.0, 300.0));
    app
}

/// Screen (Y-down) point on the first label, near the card's top-left.
const ON_LABEL: (f64, f64) = (14.0, 14.0);
/// Well below the card.
const OFF: (f64, f64) = (150.0, 290.0);

fn click(app: &mut App, at: (f64, f64)) {
    app.on_mouse_move(at.0, at.1);
    app.on_mouse_down(at.0, at.1, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(at.0, at.1, MouseButton::Left, Modifiers::default());
}

fn the_clickable(app: &App) -> &Clickable {
    let w = find_widget_by_id(app.root(), "card").expect("card is named");
    w.as_any()
        .and_then(|a| a.downcast_ref())
        .expect("a Clickable")
}

#[test]
fn clicking_a_child_of_the_card_fires_on_click_once() {
    let hits = Rc::new(Cell::new(0));
    let mut app = app_with(
        Clickable::new(Box::new(card()))
            .with_name("card")
            .on_click(counter(&hits)),
    );
    click(&mut app, ON_LABEL);
    assert_eq!(hits.get(), 1);
    // The card wraps its content: a click below it misses.
    click(&mut app, OFF);
    assert_eq!(hits.get(), 1);
}

#[test]
fn a_release_off_the_card_does_not_click() {
    let hits = Rc::new(Cell::new(0));
    let mut app = app_with(Clickable::new(Box::new(card())).on_click(counter(&hits)));
    app.on_mouse_move(ON_LABEL.0, ON_LABEL.1);
    app.on_mouse_down(
        ON_LABEL.0,
        ON_LABEL.1,
        MouseButton::Left,
        Modifiers::default(),
    );
    app.on_mouse_up(OFF.0, OFF.1, MouseButton::Left, Modifiers::default());
    assert_eq!(hits.get(), 0);
}

#[test]
fn the_card_is_hovered_while_the_pointer_is_over_any_child() {
    let mut app = app_with(Clickable::new(Box::new(card())).with_name("card"));
    app.on_mouse_move(ON_LABEL.0, ON_LABEL.1);
    assert!(the_clickable(&app).is_hovered());
    app.on_mouse_move(OFF.0, OFF.1);
    assert!(!the_clickable(&app).is_hovered());
}

#[test]
fn the_card_sizes_to_its_content() {
    let mut expected_card = card();
    let expected = expected_card.layout(Size::new(300.0, 300.0));
    let mut clickable = Clickable::new(Box::new(card()));
    let size = clickable.layout(Size::new(300.0, 300.0));
    assert_eq!(size, expected);
    assert_eq!(
        clickable.children()[0].bounds(),
        Rect::new(0.0, 0.0, size.width, size.height)
    );
}

#[test]
fn tab_focuses_the_card_and_enter_or_space_activate_it() {
    let hits = Rc::new(Cell::new(0));
    let mut app = app_with(Clickable::new(Box::new(card())).on_click(counter(&hits)));
    app.on_key_down(Key::Tab, Modifiers::default());
    assert_eq!(app.focused_widget_type_name(), Some("Clickable"));
    app.on_key_down(Key::Enter, Modifiers::default());
    app.on_key_down(Key::Char(' '), Modifiers::default());
    assert_eq!(hits.get(), 2);
}

#[test]
fn a_disabled_card_neither_clicks_nor_takes_focus() {
    let hits = Rc::new(Cell::new(0));
    let mut app = app_with(
        Clickable::new(Box::new(card()))
            .with_enabled_fn(|| false)
            .on_click(counter(&hits)),
    );
    click(&mut app, ON_LABEL);
    app.on_key_down(Key::Tab, Modifiers::default());
    app.on_key_down(Key::Enter, Modifiers::default());
    assert_eq!(hits.get(), 0);
    assert_ne!(app.focused_widget_type_name(), Some("Clickable"));
}

#[test]
fn hover_paints_a_highlight_over_the_card() {
    let paint = |hover: bool| {
        let mut c = Clickable::new(Box::new(SizedBox::new().with_width(20.0).with_height(20.0)));
        c.layout(Size::new(20.0, 20.0));
        c.set_bounds(Rect::new(0.0, 0.0, 20.0, 20.0));
        if hover {
            c.on_event(&Event::MouseMove {
                pos: Point::new(10.0, 10.0),
            });
        }
        let mut fb = Framebuffer::new(20, 20);
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        c.paint(&mut ctx);
        c.paint_overlay(&mut ctx);
        drop(ctx);
        sample(&fb, 10, 10)
    };
    assert_eq!(
        paint(false),
        [255, 255, 255, 255],
        "idle card paints nothing"
    );
    assert_ne!(
        paint(true),
        [255, 255, 255, 255],
        "hovered card is highlighted"
    );
}
