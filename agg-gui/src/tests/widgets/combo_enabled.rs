//! `ComboBox::with_enabled_fn` and `ComboBoxDisabledStyle`.
//!
//! A disabled combo reports `is_enabled() == false`, is not focusable,
//! refuses to open from a click or a key, drops pointer and keyboard input,
//! closes a list that was open when it became disabled, and paints its
//! outline at alpha 30 and its label at alpha 50 over an unchanged fill, as
//! agg-sharp's `DropDownList` does.

use super::*;
use crate::text::Font;
use crate::theme::{current_visuals, set_visuals};
use crate::widget::{paint_subtree, Widget};
use crate::widgets::combo_box::{ComboBoxDisabledStyle, ComboBoxStyle};
use crate::{ComboBox, Event, EventResult, Key, Point};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

const W: f64 = 160.0;
const WHITE: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

/// A combo gated on the returned flag (starts enabled).
fn gated() -> (ComboBox, Rc<Cell<bool>>) {
    let enabled = Rc::new(Cell::new(true));
    let flag = Rc::clone(&enabled);
    let combo = ComboBox::new(vec!["Alpha", "Beta", "Gamma"], 0, font())
        .with_enabled_fn(move || flag.get());
    (combo, enabled)
}

fn mouse_down(pos: Point) -> Event {
    Event::MouseDown {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    }
}

fn key(key: Key) -> Event {
    Event::KeyDown {
        key,
        modifiers: Modifiers::default(),
    }
}

/// Black fill, white square outline, white text: on a black background the
/// outline and the glyphs show only as much as their alpha.
fn white_on_black(combo: ComboBox) -> ComboBox {
    combo.with_style(ComboBoxStyle {
        fill: Some(Color::black()),
        border: Some(WHITE),
        radius: Some(0.0),
        ..Default::default()
    })
}

fn render(combo: &mut ComboBox) -> Framebuffer {
    let mut v = current_visuals();
    v.text_color = WHITE;
    set_visuals(v);
    combo.layout(Size::new(W, 30.0));
    let mut fb = Framebuffer::new(W as u32, 30);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        paint_subtree(combo, &mut ctx);
    }
    fb
}

fn red(fb: &Framebuffer, x: u32, y: u32) -> u8 {
    fb.pixels()[((y * fb.width() + x) * 4) as usize]
}

/// Brightest red channel over the label's area (left of the arrow).
fn brightest_label_pixel(fb: &Framebuffer) -> u8 {
    let mut max = 0;
    for y in 4..20 {
        for x in 6..(W as u32 - 24) {
            max = max.max(red(fb, x, y));
        }
    }
    max
}

#[test]
fn combo_reports_its_enabled_predicate() {
    let (combo, enabled) = gated();
    let combo: Box<dyn Widget> = Box::new(combo);
    assert!(combo.is_enabled());
    assert!(combo.is_focusable());
    enabled.set(false);
    assert!(!combo.is_enabled());
    assert!(!combo.is_focusable());
}

#[test]
fn combo_without_enabled_fn_is_enabled() {
    let combo = ComboBox::new(vec!["Alpha"], 0, font());
    assert!(combo.is_enabled());
}

#[test]
fn disabled_combo_refuses_to_open_from_a_click_or_a_key() {
    let (mut c, enabled) = gated();
    enabled.set(false);
    c.layout(Size::new(W, 30.0));
    assert_eq!(
        c.on_event(&mouse_down(Point::new(4.0, 12.0))),
        EventResult::Ignored
    );
    assert!(!c.is_open());
    assert_eq!(c.on_event(&key(Key::Enter)), EventResult::Ignored);
    assert_eq!(c.on_event(&key(Key::Char(' '))), EventResult::Ignored);
    assert!(!c.is_open());
    assert!(!c.has_active_modal());

    enabled.set(true);
    c.on_event(&mouse_down(Point::new(4.0, 12.0)));
    assert!(c.is_open(), "opens again once enabled");
}

#[test]
fn disabled_combo_drops_selection_keys_and_wheel() {
    let changes = Rc::new(Cell::new(0));
    let count = Rc::clone(&changes);
    let (c, enabled) = gated();
    let mut c = c.on_change(move |_| count.set(count.get() + 1));
    enabled.set(false);
    c.layout(Size::new(W, 30.0));
    assert_eq!(c.on_event(&key(Key::ArrowDown)), EventResult::Ignored);
    assert_eq!(
        c.on_event(&Event::MouseWheel {
            pos: Point::new(4.0, 12.0),
            delta_x: 0.0,
            delta_y: -1.0,
            modifiers: Modifiers::default(),
        }),
        EventResult::Ignored
    );
    assert_eq!(c.selected(), 0);
    assert_eq!(changes.get(), 0);
}

#[test]
fn combo_closes_its_list_when_disabled_while_open() {
    let (mut c, enabled) = gated();
    c.layout(Size::new(W, 30.0));
    c.on_event(&mouse_down(Point::new(4.0, 12.0)));
    assert!(c.is_open());
    assert!(c.has_active_modal());

    enabled.set(false);
    assert!(!c.has_active_modal(), "a disabled combo holds no modal");
    c.layout(Size::new(W, 30.0));
    assert!(!c.is_open(), "the next layout closes the list");
}

#[test]
fn disabled_combo_closes_its_list_on_the_next_event() {
    let (mut c, enabled) = gated();
    c.layout(Size::new(W, 30.0));
    c.on_event(&mouse_down(Point::new(4.0, 12.0)));
    enabled.set(false);
    c.on_event(&Event::MouseMove {
        pos: Point::new(4.0, 40.0),
    });
    assert!(!c.is_open());
}

#[test]
fn disabled_combo_paints_faded_outline_and_label_over_the_same_fill() {
    let (c, enabled) = gated();
    let mut c = white_on_black(c);
    let on = render(&mut c);
    enabled.set(false);
    let off = render(&mut c);

    // Outline: the 1 px stroke on the edge pixel covers half of it, so the
    // disabled pixel is the enabled one scaled by 30 / 255.
    let edge_on = red(&on, 0, 12) as i32;
    assert!(edge_on > 100, "enabled outline: {edge_on}");
    let edge = red(&off, 0, 12) as i32;
    let want = edge_on * 30 / 255;
    assert!(
        (edge - want).abs() <= 2,
        "disabled outline at alpha 30: {edge}, want {want}"
    );

    // Fill unchanged.
    assert_eq!(red(&on, W as u32 / 2, 2), red(&off, W as u32 / 2, 2));

    // Label: full white enabled, at most alpha 50 of white disabled.
    let label_on = brightest_label_pixel(&on);
    let label_off = brightest_label_pixel(&off) as i32;
    assert!(label_on > 200, "enabled label: {label_on}");
    assert!(
        (40..=52).contains(&label_off),
        "disabled label at alpha 50: {label_off}"
    );
}

#[test]
fn disabled_style_overrides_the_alphas() {
    let (c, enabled) = gated();
    let mut c = white_on_black(c).with_disabled_style(ComboBoxDisabledStyle {
        border_alpha: 128,
        label_alpha: 200,
    });
    assert_eq!(c.disabled_style().border_alpha, 128);
    let edge_on = red(&render(&mut c), 0, 12) as i32;
    enabled.set(false);
    let off = render(&mut c);
    let edge = red(&off, 0, 12) as i32;
    let want = edge_on * 128 / 255;
    assert!(
        (edge - want).abs() <= 2,
        "outline at alpha 128: {edge}, want {want}"
    );
    let label = brightest_label_pixel(&off) as i32;
    assert!((150..=202).contains(&label), "label at alpha 200: {label}");
}

#[test]
fn disabled_style_defaults_match_drop_down_list() {
    let d = ComboBoxDisabledStyle::default();
    assert_eq!((d.border_alpha, d.label_alpha), (30, 50));
}
