//! agg-sharp `ThemedTextEditWidgetColorTests`
//! (Tests/Agg.Tests/Agg.UI/ThemedTextEditWidgetColorTests.cs): a themed
//! field shows the theme's edit-field colours until the caller sets one, then
//! the one the caller set.
//!
//! agg-sharp's `ThemedTextEditWidget` is agg-gui's framed [`TextField`]; its
//! `ThemeConfig.EditFieldColors.Inactive` are the ambient visuals
//! (`widget_bg`, `widget_stroke`), and a caller's `BorderColor` /
//! `BackgroundColor` are [`TextFieldTheme`] overrides. C# reads the colours
//! back from properties; agg-gui keeps no reported colour, so these tests read
//! the colour the field actually paints its border and background in, through
//! the production `paint_subtree` path into a software `GfxCtx`.

use std::sync::Arc;

use super::*;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::widget::{paint_subtree, Widget};

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

const W: u32 = 120;
const H: u32 = 30;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Paint `f`, laid out at `W` x `H`, unfocused and unhovered (the inactive
/// colours), over a host colour no theme uses.
fn render(f: &mut TextField) -> Framebuffer {
    let _profile = crate::input_profile::profile_test_lock();
    crate::device_scale::set_device_scale(1.0);
    crate::font_settings::set_lcd_enabled(false);
    f.set_bounds(Rect::new(0.0, 0.0, W as f64, H as f64));
    f.layout(Size::new(W as f64, H as f64));
    f.set_bounds(Rect::new(0.0, 0.0, W as f64, H as f64));
    let mut fb = Framebuffer::new(W, H);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgb(1.0, 0.0, 1.0));
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

/// `top` source-over an opaque `bottom`, as 8-bit RGB.
fn over(top: Color, bottom: Color) -> [u8; 3] {
    let (t, b) = (top.to_rgba8(), bottom.to_rgba8());
    let a = t.a as f64 / 255.0;
    let mix = |x: u8, y: u8| (x as f64 * a + y as f64 * (1.0 - a)).round() as u8;
    [mix(t.r, b.r), mix(t.g, b.g), mix(t.b, b.b)]
}

fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2)
}

/// The colour of the middle of the bottom border row (the one-pixel inactive
/// stroke covers row 0 exactly) and of the field's centre.
fn border_and_background(fb: &Framebuffer) -> ([u8; 3], [u8; 3]) {
    (rgb(fb, W / 2, 0), rgb(fb, W / 2, H / 2))
}

#[test]
fn explicit_border_color_is_the_border_color_reported() {
    let blue = Color::rgb(0.0, 0.0, 1.0);
    let red = Color::rgb(1.0, 0.0, 0.0);
    let mut field = TextField::new(font()).with_theme(TextFieldTheme {
        background: Some(blue),
        border_color: Some(red),
        ..TextFieldTheme::default()
    });

    let (border, background) = border_and_background(&render(&mut field));

    assert!(
        close(border, rgb_of(red)),
        "border painted {border:?}, expected the caller's red"
    );
    assert!(
        close(background, rgb_of(blue)),
        "background painted {background:?}, expected the caller's blue"
    );
}

#[test]
fn unset_border_color_follows_the_theme() {
    let mut field = TextField::new(font());

    let (border, _) = border_and_background(&render(&mut field));

    // The theme's border stroke may be translucent (the dark palette's is),
    // so what shows is that stroke laid over the field's own background.
    let v = crate::theme::current_visuals();
    let theme_border = over(v.widget_stroke, v.widget_bg);
    assert!(
        close(border, theme_border),
        "border painted {border:?}, expected the theme's inactive edit-field border {theme_border:?}"
    );
}
