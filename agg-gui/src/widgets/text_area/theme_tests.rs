//! Pixel tests for [`TextArea::with_theme`] (`frame.rs`): the per-state
//! background, border and text colours of agg-sharp's
//! `ThemeConfig.EditFieldColors` as MatterCAD's multi-line
//! `ThemedTextEditWidget` paints them (focus over hover over normal), the
//! focused border width, the hint colours, and a live `set_theme`. The same
//! [`TextFieldTheme`](crate::widgets::TextFieldTheme) themes a `TextField`.
//!
//! Each test paints the production widget through `paint_subtree` into a
//! software `GfxCtx` and reads back the colours it drew.

use std::sync::Arc;

use super::*;
use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::TextFieldTheme;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

const W: u32 = 200;
const H: u32 = 100;

#[derive(Clone, Copy)]
pub(super) enum State {
    Normal,
    Hovered,
    Focused,
}

fn c(r: f32, g: f32, b: f32) -> Color {
    Color::rgb(r, g, b)
}

/// One distinct colour per state and role, none equal to another.
pub(super) fn colors() -> TextFieldTheme {
    TextFieldTheme {
        background: Some(c(0.1, 0.1, 0.4)),
        background_hovered: Some(c(0.1, 0.4, 0.1)),
        background_focused: Some(c(0.4, 0.1, 0.1)),
        text_color: Some(c(1.0, 1.0, 0.0)),
        text_color_hovered: Some(c(0.0, 1.0, 1.0)),
        text_color_focused: Some(c(1.0, 1.0, 1.0)),
        placeholder_color: Some(c(0.0, 0.0, 1.0)),
        placeholder_color_focused: Some(c(0.0, 1.0, 0.0)),
        border_color: Some(c(0.9, 0.5, 0.0)),
        border_color_hovered: Some(c(0.0, 0.5, 0.9)),
        border_color_focused: Some(c(0.5, 0.9, 0.0)),
        ..TextFieldTheme::default()
    }
}

pub(super) fn area(text: &str, hint: &str, theme: TextFieldTheme) -> TextArea {
    TextArea::new(Arc::new(Font::from_slice(FONT_BYTES).expect("font")))
        .with_font_size(24.0)
        .with_text(text)
        .with_hint_text(hint)
        .with_theme(theme)
}

/// Put `ta` in `state`, lay it out at `W` x `H` and paint it over magenta.
pub(super) fn render(ta: &mut TextArea, state: State) -> Framebuffer {
    let _profile = crate::input_profile::profile_test_lock();
    crate::device_scale::set_device_scale(1.0);
    crate::font_settings::set_lcd_enabled(false);
    ta.set_bounds(Rect::new(0.0, 0.0, W as f64, H as f64));
    ta.layout(Size::new(W as f64, H as f64));
    ta.set_bounds(Rect::new(0.0, 0.0, W as f64, H as f64));
    match state {
        State::Normal => {}
        State::Hovered => {
            ta.on_event(&Event::MouseMove {
                pos: Point::new(W as f64 / 2.0, H as f64 / 2.0),
            });
        }
        State::Focused => {
            ta.on_event(&Event::FocusGained);
        }
    }
    ta.layout(Size::new(W as f64, H as f64));
    ta.set_bounds(Rect::new(0.0, 0.0, W as f64, H as f64));
    let mut fb = Framebuffer::new(W, H);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(c(1.0, 0.0, 1.0));
        paint_subtree(ta, &mut ctx);
    }
    crate::font_settings::clear_lcd_enabled_override();
    fb
}

/// RGB of the pixel at `(x, y)`, Y-up (row 0 is the bottom).
pub(super) fn rgb(fb: &Framebuffer, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2]]
}

pub(super) fn rgb_of(c: Color) -> [u8; 3] {
    let c = c.to_rgba8();
    [c.r, c.g, c.b]
}

pub(super) fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2)
}

/// The bottom border row's middle pixel and a pixel low in the empty body.
fn border_and_background(fb: &Framebuffer) -> ([u8; 3], [u8; 3]) {
    (rgb(fb, W / 2, 0), rgb(fb, W / 2, 20))
}

/// The pixel inside the border farthest from `background`.
pub(super) fn strongest_pixel(fb: &Framebuffer, background: [u8; 3]) -> [u8; 3] {
    let dist = |p: [u8; 3]| -> u32 {
        p.iter()
            .zip(background)
            .map(|(a, b)| a.abs_diff(b) as u32)
            .sum()
    };
    let mut best = background;
    for y in 4..H - 4 {
        for x in 4..W - 4 {
            let p = rgb(fb, x, y);
            if dist(p) > dist(best) {
                best = p;
            }
        }
    }
    best
}

fn assert_state(state: State, border: Color, background: Color, text: Color) {
    let mut theme = colors();
    // The caret paints in the text colour, so it cannot be the strongest
    // pixel unless that is right too.
    theme.cursor_color = Some(text);
    let mut ta = area("", "", theme);
    let (b, bg) = border_and_background(&render(&mut ta, state));
    assert!(close(b, rgb_of(border)), "border painted {b:?}");
    assert!(close(bg, rgb_of(background)), "background painted {bg:?}");
    let mut ta = area("MMMM", "", theme);
    let p = strongest_pixel(&render(&mut ta, state), rgb_of(background));
    assert!(close(p, rgb_of(text)), "text painted {p:?}");
}

#[test]
fn rust_only_unfocused_area_paints_the_inactive_edit_field_colors() {
    let t = colors();
    let (b, bg, tx) = (t.border_color, t.background, t.text_color);
    assert_state(State::Normal, b.unwrap(), bg.unwrap(), tx.unwrap());
}

#[test]
fn rust_only_hovered_area_paints_the_hovered_edit_field_colors() {
    let t = colors();
    let (b, bg) = (t.border_color_hovered, t.background_hovered);
    let tx = t.text_color_hovered;
    assert_state(State::Hovered, b.unwrap(), bg.unwrap(), tx.unwrap());
}

#[test]
fn rust_only_focused_area_paints_the_focused_edit_field_colors() {
    let t = colors();
    let (b, bg) = (t.border_color_focused, t.background_focused);
    let tx = t.text_color_focused;
    assert_state(State::Focused, b.unwrap(), bg.unwrap(), tx.unwrap());
}

#[test]
fn rust_only_focused_area_border_width_follows_the_theme() {
    let t = colors();
    let mut ta = area("", "", t);
    let fb = render(&mut ta, State::Focused);
    let p = rgb(&fb, W / 2, 1);
    let border = rgb_of(t.border_color_focused.unwrap());
    assert!(close(p, border), "default 2 px ring: second row {p:?}");

    let thin = TextFieldTheme {
        border_width_focused: Some(1.0),
        ..t
    };
    let mut ta = area("", "", thin);
    let fb = render(&mut ta, State::Focused);
    assert!(close(rgb(&fb, W / 2, 0), border), "1 px ring");
    let p = rgb(&fb, W / 2, 1);
    let bg = rgb_of(t.background_focused.unwrap());
    assert!(close(p, bg), "1 px ring: second row {p:?}");
}

#[test]
fn rust_only_area_hint_takes_the_state_hint_color() {
    for (state, background, hint) in [
        (
            State::Normal,
            colors().background,
            colors().placeholder_color,
        ),
        (
            State::Focused,
            colors().background_focused,
            colors().placeholder_color_focused,
        ),
    ] {
        let background = background.unwrap();
        let theme = TextFieldTheme {
            cursor_color: Some(background),
            ..colors()
        };
        let mut ta = area("", "MMMM", theme);
        let p = strongest_pixel(&render(&mut ta, state), rgb_of(background));
        let hint = rgb_of(hint.unwrap());
        assert!(close(p, hint), "hint painted {p:?}, expected {hint:?}");
    }
}

#[test]
fn rust_only_area_set_theme_repaints_a_laid_out_area() {
    let mut ta = area("", "", TextFieldTheme::default());
    render(&mut ta, State::Normal);
    let t = colors();
    ta.set_theme(t);
    assert_eq!(ta.theme().background, t.background);
    let (b, bg) = border_and_background(&render(&mut ta, State::Normal));
    assert!(close(b, rgb_of(t.border_color.unwrap())), "border {b:?}");
    assert!(
        close(bg, rgb_of(t.background.unwrap())),
        "background {bg:?}"
    );
}
