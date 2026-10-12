//! Pixel tests for [`TextFieldTheme`]'s per-state colours, agg-sharp's
//! `ThemeConfig.EditFieldColors` (`Inactive`, `Hovered`, `Focused`) as
//! `ThemedTextEditWidget` paints them: the background and border follow the
//! field's state (focus over hover), and the text switches to the focused
//! text colour while the field holds focus.
//!
//! Each test paints through the production `paint_subtree` path into a
//! software `GfxCtx`, as `themed_text_edit_widget_color_tests` does, and
//! reads back the colours the field actually drew.

use std::sync::Arc;

use super::*;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::widget::{paint_subtree, Widget};
use crate::Point;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

const W: u32 = 160;
const H: u32 = 40;

#[derive(Clone, Copy)]
enum State {
    Normal,
    Hovered,
    Focused,
}

fn c(r: f32, g: f32, b: f32) -> Color {
    Color::rgb(r, g, b)
}

/// One distinct colour per state and role, none equal to another.
fn edit_field_colors() -> TextFieldTheme {
    TextFieldTheme {
        background: Some(c(0.1, 0.1, 0.4)),
        background_hovered: Some(c(0.1, 0.4, 0.1)),
        background_focused: Some(c(0.4, 0.1, 0.1)),
        text_color: Some(c(1.0, 1.0, 0.0)),
        text_color_hovered: Some(c(0.0, 1.0, 1.0)),
        text_color_focused: Some(c(1.0, 1.0, 1.0)),
        border_color: Some(c(0.9, 0.5, 0.0)),
        border_color_hovered: Some(c(0.0, 0.5, 0.9)),
        border_color_focused: Some(c(0.5, 0.9, 0.0)),
        ..TextFieldTheme::default()
    }
}

fn field(text: &str, theme: TextFieldTheme) -> TextField {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    TextField::new(font)
        .with_font_size(24.0)
        .with_text(text)
        .with_theme(theme)
}

/// Put `f` in `state`, lay it out at `W` x `H` and paint it over magenta.
fn render(f: &mut TextField, state: State) -> Framebuffer {
    let _profile = crate::input_profile::profile_test_lock();
    crate::device_scale::set_device_scale(1.0);
    crate::font_settings::set_lcd_enabled(false);
    f.set_bounds(Rect::new(0.0, 0.0, W as f64, H as f64));
    match state {
        State::Normal => {}
        State::Hovered => {
            f.on_event(&Event::MouseMove {
                pos: Point::new(W as f64 / 2.0, H as f64 / 2.0),
            });
        }
        State::Focused => {
            f.on_event(&Event::FocusGained);
        }
    }
    f.layout(Size::new(W as f64, H as f64));
    f.set_bounds(Rect::new(0.0, 0.0, W as f64, H as f64));
    let mut fb = Framebuffer::new(W, H);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(c(1.0, 0.0, 1.0));
        paint_subtree(f, &mut ctx);
    }
    crate::font_settings::clear_lcd_enabled_override();
    fb
}

fn rgb(fb: &Framebuffer, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2]]
}

fn rgb_of(c: Color) -> [u8; 3] {
    let c = c.to_rgba8();
    [c.r, c.g, c.b]
}

fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2)
}

/// The bottom border row's middle pixel (fully covered by the one-pixel
/// stroke, and by the two-pixel focused one) and the field's centre.
fn border_and_background(fb: &Framebuffer) -> ([u8; 3], [u8; 3]) {
    (rgb(fb, W / 2, 0), rgb(fb, W / 2, H / 2))
}

/// The text-area pixel farthest from `background`: the core of a glyph stem.
fn strongest_text_pixel(fb: &Framebuffer, background: [u8; 3]) -> [u8; 3] {
    let dist = |p: [u8; 3]| -> u32 {
        p.iter()
            .zip(background)
            .map(|(a, b)| a.abs_diff(b) as u32)
            .sum()
    };
    let mut best = background;
    for y in 4..H - 4 {
        for x in 8..W - 8 {
            let p = rgb(fb, x, y);
            if dist(p) > dist(best) {
                best = p;
            }
        }
    }
    best
}

fn assert_frame(state: State, border: Option<Color>, background: Option<Color>) {
    let theme = edit_field_colors();
    let mut f = field("", theme);
    let (b, bg) = border_and_background(&render(&mut f, state));
    let (border, background) = (border.unwrap(), background.unwrap());
    assert!(
        close(b, rgb_of(border)),
        "border painted {b:?}, expected {:?}",
        rgb_of(border)
    );
    assert!(
        close(bg, rgb_of(background)),
        "background painted {bg:?}, expected {:?}",
        rgb_of(background)
    );
}

fn assert_text(state: State, text: Option<Color>, background: Option<Color>) {
    let mut f = field("MMMM", edit_field_colors());
    let fb = render(&mut f, state);
    let p = strongest_text_pixel(&fb, rgb_of(background.unwrap()));
    let text = rgb_of(text.unwrap());
    assert!(close(p, text), "text painted {p:?}, expected {text:?}");
}

#[test]
fn rust_only_unfocused_field_paints_the_inactive_edit_field_colors() {
    let t = edit_field_colors();
    assert_frame(State::Normal, t.border_color, t.background);
    assert_text(State::Normal, t.text_color, t.background);
}

#[test]
fn rust_only_hovered_field_paints_the_hovered_edit_field_colors() {
    let t = edit_field_colors();
    assert_frame(State::Hovered, t.border_color_hovered, t.background_hovered);
    assert_text(State::Hovered, t.text_color_hovered, t.background_hovered);
}

#[test]
fn rust_only_focused_field_paints_the_focused_edit_field_colors() {
    let t = edit_field_colors();
    assert_frame(State::Focused, t.border_color_focused, t.background_focused);
    // The caret paints in the focused text colour too, so it cannot be the
    // strongest pixel unless it matches.
    let mut theme = t;
    theme.cursor_color = t.text_color_focused;
    let mut f = field("MMMM", theme);
    let fb = render(&mut f, State::Focused);
    let p = strongest_text_pixel(&fb, rgb_of(t.background_focused.unwrap()));
    let text = rgb_of(t.text_color_focused.unwrap());
    assert!(close(p, text), "text painted {p:?}, expected {text:?}");
}

#[test]
fn rust_only_unset_state_colors_fall_back_to_the_normal_colors() {
    // A theme with only the normal background and text keeps them in every
    // state (the border keeps its visuals hover and focus colours).
    let theme = TextFieldTheme {
        background: Some(c(0.1, 0.1, 0.4)),
        text_color: Some(c(1.0, 1.0, 0.0)),
        cursor_color: Some(c(1.0, 1.0, 0.0)),
        ..TextFieldTheme::default()
    };
    for state in [State::Normal, State::Hovered, State::Focused] {
        let mut f = field("", theme);
        let (_, bg) = border_and_background(&render(&mut f, state));
        assert!(
            close(bg, rgb_of(theme.background.unwrap())),
            "background {bg:?}"
        );
        let mut f = field("MMMM", theme);
        let fb = render(&mut f, state);
        let p = strongest_text_pixel(&fb, rgb_of(theme.background.unwrap()));
        assert!(close(p, rgb_of(theme.text_color.unwrap())), "text {p:?}");
    }
}

#[test]
fn rust_only_set_theme_repaints_a_laid_out_field() {
    let mut f = field("", TextFieldTheme::default());
    render(&mut f, State::Normal);
    let t = edit_field_colors();
    f.set_theme(t);
    let (b, bg) = border_and_background(&render(&mut f, State::Normal));
    assert!(close(b, rgb_of(t.border_color.unwrap())), "border {b:?}");
    assert!(
        close(bg, rgb_of(t.background.unwrap())),
        "background {bg:?}"
    );
}
