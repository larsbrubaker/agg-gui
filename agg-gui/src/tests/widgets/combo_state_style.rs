//! `ComboBox::with_state_style` and the "no selection" state.
//!
//! Paints real `ComboBox`es into a software framebuffer and samples pixels
//! to prove each `ComboBoxStateStyle` override (hover / focus outline, open
//! fill, list fill, item text, hovered item) is honoured while an unstyled
//! combo keeps its rest outline, and drives `with_no_selection` through
//! clicks, keys and the API.

use super::*;
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::combo_box::{ComboBoxStateStyle, ComboBoxStyle};
use crate::{ComboBox, Event, Key, Point};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

const W: f64 = 160.0;
const RED: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

fn combo() -> ComboBox {
    ComboBox::new(vec!["Alpha", "Beta", "Gamma"], 0, font())
}

fn render(combo: &mut ComboBox) -> Framebuffer {
    combo.layout(Size::new(W, 30.0));
    let mut fb = Framebuffer::new(W as u32, 30);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        paint_subtree(combo, &mut ctx);
    }
    fb
}

fn sample(fb: &Framebuffer, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = &fb.pixels()[i..i + 4];
    [p[0], p[1], p[2]]
}

fn green_dominant(p: [u8; 3]) -> bool {
    p[1] > 100 && p[1] > p[0] && p[1] > p[2]
}

fn blue_dominant(p: [u8; 3]) -> bool {
    p[2] > 100 && p[2] > p[0] && p[2] > p[1]
}

fn mouse_down(pos: Point) -> Event {
    Event::MouseDown {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    }
}

/// Left edge of the closed box, mid-height: where the 1 px outline sits.
const EDGE: (u32, u32) = (0, 12);

#[test]
fn combo_state_style_hover_and_focus_borders() {
    let mut c = combo()
        .with_style(ComboBoxStyle {
            fill: Some(Color::black()),
            border: Some(RED),
            radius: Some(0.0),
            ..Default::default()
        })
        .with_state_style(ComboBoxStateStyle {
            hover_border: Some(Color::rgb(0.0, 1.0, 0.0)),
            focus_border: Some(Color::rgb(0.0, 0.0, 1.0)),
            ..Default::default()
        });
    let rest = sample(&render(&mut c), EDGE.0, EDGE.1);
    assert!(
        rest[0] > 100 && rest[1] < 50,
        "rest border is red: {rest:?}"
    );

    c.on_event(&Event::MouseMove {
        pos: Point::new(4.0, 12.0),
    });
    let hovered = sample(&render(&mut c), EDGE.0, EDGE.1);
    assert!(green_dominant(hovered), "hover border: {hovered:?}");

    // Focus wins over hover.
    c.on_event(&Event::FocusGained);
    let focused = sample(&render(&mut c), EDGE.0, EDGE.1);
    assert!(blue_dominant(focused), "focus border: {focused:?}");

    c.on_event(&Event::FocusLost);
    c.on_event(&Event::MouseMove {
        pos: Point::new(4.0, 100.0),
    });
    let back = sample(&render(&mut c), EDGE.0, EDGE.1);
    assert_eq!(back, rest, "rest border restored");
}

#[test]
fn combo_without_state_style_keeps_rest_border_when_hovered_or_focused() {
    let mut c = combo().with_style(ComboBoxStyle {
        border: Some(RED),
        radius: Some(0.0),
        ..Default::default()
    });
    let rest = sample(&render(&mut c), EDGE.0, EDGE.1);
    c.on_event(&Event::MouseMove {
        pos: Point::new(4.0, 12.0),
    });
    c.on_event(&Event::FocusGained);
    assert_eq!(sample(&render(&mut c), EDGE.0, EDGE.1), rest);
}

#[test]
fn combo_state_style_open_fill() {
    let mut c = combo()
        .with_style(ComboBoxStyle {
            fill: Some(Color::black()),
            ..Default::default()
        })
        .with_state_style(ComboBoxStateStyle {
            open_fill: Some(Color::rgb(0.0, 1.0, 0.0)),
            ..Default::default()
        });
    assert_eq!(sample(&render(&mut c), 4, 12), [0, 0, 0]);
    c.layout(Size::new(W, 30.0));
    c.on_event(&mouse_down(Point::new(4.0, 12.0)));
    assert!(c.is_open());
    assert_eq!(sample(&render(&mut c), 4, 12), [0, 255, 0], "open fill");
}

/// Open `combo` as the root of an `App` with the popup hanging below it, and
/// paint a frame (the popup paints in the global combo-popup pass).
fn open_in_app(combo: ComboBox, hover: Option<usize>) -> Framebuffer {
    let viewport = Size::new(W, 200.0);
    let mut app = App::new(Box::new(combo));
    app.layout(viewport);
    // The root combo sits at the bottom (Y-up); the popup opens upward, so
    // item i's centre is at y_up = 24 + (n - 1 - i) * 22 + 11 for n = 3.
    app.on_mouse_down(12.0, 200.0 - 12.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(12.0, 200.0 - 12.0, MouseButton::Left, Modifiers::default());
    let mut fb = Framebuffer::new(W as u32, 200);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        app.paint(&mut ctx);
    }
    if let Some(i) = hover {
        let y_up = item_center_y(i);
        app.on_mouse_move(W - 20.0, 200.0 - y_up);
        let mut ctx = GfxCtx::new(&mut fb);
        app.paint(&mut ctx);
    }
    fb
}

fn item_center_y(i: usize) -> f64 {
    24.0 + (2 - i) as f64 * 22.0 + 11.0
}

#[test]
fn combo_state_style_popup_fill_and_item_colours() {
    let style = ComboBoxStateStyle {
        popup_fill: Some(Color::rgb(0.0, 0.0, 1.0)),
        item_text: Some(Color::rgb(0.0, 1.0, 0.0)),
        item_hover_fill: Some(RED),
        item_hover_text: Some(Color::white()),
        ..Default::default()
    };
    let fb = open_in_app(combo().with_state_style(style), None);
    // Item 1 ("Beta", unselected, unhovered): list fill right of the text,
    // item-text green somewhere in the label.
    let y1 = item_center_y(1) as u32;
    assert_eq!(
        sample(&fb, (W - 20.0) as u32, y1),
        [0, 0, 255],
        "popup fill"
    );
    let has_green = (8..60).any(|x| green_dominant(sample(&fb, x, y1)));
    assert!(has_green, "item text colour not used");

    let fb = open_in_app(combo().with_state_style(style), Some(2));
    let y2 = item_center_y(2) as u32;
    assert_eq!(
        sample(&fb, (W - 20.0) as u32, y2),
        [255, 0, 0],
        "hover fill"
    );
    let has_white = (8..60).any(|x| sample(&fb, x, y2).iter().all(|c| *c > 200));
    assert!(has_white, "hovered item text colour not used");
}

#[test]
fn combo_default_popup_uses_visuals() {
    let v = crate::theme::current_visuals();
    let fb = open_in_app(combo(), None);
    let p = sample(&fb, (W - 20.0) as u32, item_center_y(1) as u32);
    let bg = [
        (v.widget_bg.r * 255.0).round() as u8,
        (v.widget_bg.g * 255.0).round() as u8,
        (v.widget_bg.b * 255.0).round() as u8,
    ];
    assert!(
        p.iter().zip(bg.iter()).all(|(a, b)| a.abs_diff(*b) <= 3),
        "default list fill {p:?} != widget_bg {bg:?}"
    );
}

#[test]
fn combo_no_selection_until_picked() {
    let fired = Rc::new(Cell::new(None));
    let f = Rc::clone(&fired);
    let mut c = combo()
        .with_no_selection("Choose")
        .on_change(move |i| f.set(Some(i)));
    assert_eq!(c.selected_index(), None);
    assert_eq!(c.placeholder(), "Choose");

    // The closed box shows the placeholder (some text pixels in the label).
    let fb = render(&mut c);
    let text_px = (8..60).any(|x| sample(&fb, x, 12).iter().any(|ch| *ch > 150));
    assert!(text_px, "placeholder text painted");

    // An arrow key selects the first option.
    c.on_event(&Event::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::default(),
    });
    assert_eq!(c.selected_index(), Some(0));
    assert_eq!(fired.get(), Some(0));

    c.clear_selection();
    assert_eq!(c.selected_index(), None);
    c.set_selected_index(Some(2));
    assert_eq!((c.selected_index(), c.selected()), (Some(2), 2));
    c.set_selected_index(None);
    assert_eq!(c.selected_index(), None);

    // A plain combo always has a selection; an empty one with a placeholder
    // shows it without panicking.
    assert_eq!(combo().selected_index(), Some(0));
    let mut empty = ComboBox::new(Vec::<String>::new(), 0, font()).with_no_selection("Nothing yet");
    render(&mut empty);
    empty.on_event(&Event::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::default(),
    });
    assert_eq!(empty.selected_index(), None);
}

#[test]
fn combo_no_selection_has_no_highlighted_row_and_click_selects() {
    let accent = crate::theme::current_visuals().accent;
    let accent = [
        (accent.r * 255.0).round() as u8,
        (accent.g * 255.0).round() as u8,
        (accent.b * 255.0).round() as u8,
    ];
    let near = |p: [u8; 3]| {
        p.iter()
            .zip(accent.iter())
            .all(|(a, b)| a.abs_diff(*b) <= 3)
    };
    let fb = open_in_app(combo(), None);
    assert!(near(sample(
        &fb,
        (W - 20.0) as u32,
        item_center_y(0) as u32
    )));
    let fb = open_in_app(combo().with_no_selection("Choose"), None);
    for i in 0..3 {
        let p = sample(&fb, (W - 20.0) as u32, item_center_y(i) as u32);
        assert!(!near(p), "row {i} highlighted with no selection: {p:?}");
    }

    let picked = Rc::new(Cell::new(None));
    let p = Rc::clone(&picked);
    let mut app = App::new(Box::new(
        combo()
            .with_no_selection("Choose")
            .on_change(move |i| p.set(Some(i))),
    ));
    app.layout(Size::new(W, 200.0));
    app.on_mouse_down(12.0, 188.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(12.0, 188.0, MouseButton::Left, Modifiers::default());
    let mut fb = Framebuffer::new(W as u32, 200);
    app.paint(&mut GfxCtx::new(&mut fb));
    let y = 200.0 - item_center_y(1);
    app.on_mouse_down(12.0, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(12.0, y, MouseButton::Left, Modifiers::default());
    assert_eq!(picked.get(), Some(1));
}
