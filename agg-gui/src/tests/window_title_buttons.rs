//! `Window` title-bar buttons and whole-window opacity — the agg-gui side of
//! agg-sharp's `WindowWidget.AddTitleBarButton` and
//! `GuiWidget.BackbufferOpacity` on a double-buffered window.
//!
//! Covers placement (left of the maximize / close buttons, later buttons
//! nearer close), the real App pointer path (a press and release on the
//! button clicks it; hovering it shows its tooltip), the opacity setter's
//! clamp, and a software-ctx pixel test that a window at opacity 0.6 blends
//! over what is behind it.

use super::*;
use crate::event::{Modifiers, MouseButton};
use crate::geometry::{Point, Rect};
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::tooltip::controller;
use crate::widgets::tooltip::{reset_tooltip_test_state, tooltip_timings};
use crate::widgets::window::Window;
use crate::Stack;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

const VP_W: f64 = 400.0;
const VP_H: f64 = 300.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

fn small_button(label: &str, font: &Arc<Font>) -> Button {
    Button::new(label, Arc::clone(font))
        .with_font_size(11.0)
        .with_compact()
}

/// The maximize button's circle reaches this far left of the window's right
/// edge (its centre is 26 px in, radius 6).
const MAXIMIZE_LEFT_INSET: f64 = 32.0;
const TITLE_H: f64 = 28.0;

#[test]
fn title_bar_button_sits_left_of_maximize_and_close() {
    let font = font();
    let mut win = Window::new("Buttons", Arc::clone(&font), Box::new(SizedBox::new()))
        .with_bounds(Rect::new(0.0, 0.0, 300.0, 200.0))
        .with_title_bar_button(Box::new(small_button("T", &font)));
    win.layout(Size::new(VP_W, VP_H));

    let r = win.title_bar_button_rect(0).expect("button rect");
    assert!(r.width > 0.0 && r.height > 0.0, "button laid out: {r:?}");
    assert!(
        r.x + r.width <= 300.0 - MAXIMIZE_LEFT_INSET,
        "button must end left of the maximize button: {r:?}"
    );
    assert!(
        r.y >= 200.0 - TITLE_H && r.y + r.height <= 200.0,
        "button must sit inside the title bar: {r:?}"
    );
    assert_eq!(win.title_bar_buttons().len(), 1);
}

#[test]
fn later_title_bar_button_sits_nearer_close() {
    let font = font();
    let mut win = Window::new("Buttons", Arc::clone(&font), Box::new(SizedBox::new()))
        .with_bounds(Rect::new(0.0, 0.0, 300.0, 200.0))
        .with_title_bar_button(Box::new(small_button("A", &font)))
        .with_title_bar_button(Box::new(small_button("B", &font)));
    win.layout(Size::new(VP_W, VP_H));

    let first = win.title_bar_button_rect(0).unwrap();
    let second = win.title_bar_button_rect(1).unwrap();
    assert!(
        first.x + first.width <= second.x,
        "first-added button left of the later one: {first:?} {second:?}"
    );
}

fn build_app(font: &Arc<Font>, clicks: Rc<Cell<u32>>) -> App {
    let button = {
        let clicks = Rc::clone(&clicks);
        small_button("T", font)
            .with_tooltip("Make the window transparent")
            .on_click(move || clicks.set(clicks.get() + 1))
    };
    let win = Window::new("Buttons", Arc::clone(font), Box::new(SizedBox::new()))
        .with_bounds(Rect::new(40.0, 40.0, 300.0, 200.0))
        .with_title_bar_button(Box::new(button));
    let mut app = App::new(Box::new(Stack::new().add(Box::new(win))));
    app.layout(Size::new(VP_W, VP_H));
    app
}

fn button_world_center(app: &App) -> Point {
    let win = app.root().children()[0]
        .as_any()
        .and_then(|a| a.downcast_ref::<Window>())
        .expect("window");
    let wb = win.bounds();
    let r = win.title_bar_button_rect(0).expect("button rect");
    Point::new(wb.x + r.x + r.width * 0.5, wb.y + r.y + r.height * 0.5)
}

#[test]
fn title_bar_button_clicks_through_the_app() {
    let font = font();
    let clicks = Rc::new(Cell::new(0));
    let mut app = build_app(&font, Rc::clone(&clicks));
    let c = button_world_center(&app);

    app.on_mouse_move(c.x, VP_H - c.y);
    app.on_mouse_down(c.x, VP_H - c.y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(c.x, VP_H - c.y, MouseButton::Left, Modifiers::default());
    assert_eq!(clicks.get(), 1, "press + release on the button clicks it");

    // The window did not start a title drag from the button press.
    let win = app.root().children()[0].bounds();
    assert_eq!((win.x, win.y), (40.0, 40.0));
}

#[test]
fn hovering_title_bar_button_shows_its_tooltip() {
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);
    reset_tooltip_test_state();
    controller::reset();
    crate::clock::start_virtual();
    crate::font_settings::set_system_font(Some(font()));

    let font = font();
    let mut app = build_app(&font, Rc::new(Cell::new(0)));
    let c = button_world_center(&app);
    app.on_mouse_move(c.x, VP_H - c.y);
    app.update_tooltips_for_test();
    crate::clock::advance(tooltip_timings().initial_delay);
    app.update_tooltips_for_test();
    let shown = controller::visible_text();

    reset_tooltip_test_state();
    controller::reset();
    crate::font_settings::set_system_font(None);

    assert_eq!(shown.as_deref(), Some("Make the window transparent"));
}

#[test]
fn backbuffer_opacity_clamps_to_unit_range() {
    let mut win = Window::new("Opacity", font(), Box::new(SizedBox::new()));
    assert_eq!(win.backbuffer_opacity(), 1.0);
    win.set_backbuffer_opacity(2.0);
    assert_eq!(win.backbuffer_opacity(), 1.0);
    win.set_backbuffer_opacity(-1.0);
    assert_eq!(win.backbuffer_opacity(), 0.0);
    win.set_backbuffer_opacity(0.6);
    assert!((win.backbuffer_opacity() - 0.6).abs() < 1e-12);
    assert!(
        (win.backbuffer_spec().alpha - 0.6).abs() < 1e-9,
        "the retained layer composites at the opacity"
    );
}

fn paint_over_red(win: &mut Window) -> [u8; 4] {
    let mut fb = Framebuffer::new(240, 180);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgb(1.0, 0.0, 0.0));
        ctx.save();
        let b = win.bounds();
        ctx.translate(b.x, b.y);
        paint_subtree(win, &mut ctx);
        ctx.restore();
    }
    // Middle of the content area (window-local (100, 50)), Y-up rows.
    let (x, y) = (120u32, 70u32);
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = &fb.pixels()[i..i + 4];
    [p[0], p[1], p[2], p[3]]
}

#[test]
fn window_at_opacity_blends_over_what_is_behind_it() {
    let mut win = Window::new("Opacity", font(), Box::new(SizedBox::new()))
        .with_bounds(Rect::new(20.0, 20.0, 200.0, 140.0));
    win.layout(Size::new(240.0, 180.0));
    let opaque = paint_over_red(&mut win);

    win.set_backbuffer_opacity(0.6);
    let faded = paint_over_red(&mut win);

    let bg = [255.0, 0.0, 0.0];
    for ch in 0..3 {
        let expected = bg[ch] + 0.6 * (opaque[ch] as f64 - bg[ch]);
        assert!(
            (faded[ch] as f64 - expected).abs() <= 3.0,
            "channel {ch}: opaque {opaque:?}, faded {faded:?}, expected {expected:.1}"
        );
    }
    assert!(
        faded[0] > opaque[0] + 40,
        "the red behind must show through: opaque {opaque:?}, faded {faded:?}"
    );
}
