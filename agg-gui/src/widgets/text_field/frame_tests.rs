//! Pixel tests for [`TextField::with_frame`] (`theme.rs`): a framed field
//! paints its background and border, a frameless one leaves the host's
//! background showing (no fill, no border, no focus ring) and draws its text
//! where a framed one does.
//!
//! Painted through the production `paint_subtree` backbuffer path into a
//! software `GfxCtx`, under both the LCD and the grayscale backbuffer modes.

use std::sync::Arc;

use super::*;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::widget::{paint_subtree, Widget};

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

const W: u32 = 200;
const H: u32 = 30;
const PADDING: f64 = 8.0;

/// A colour no theme uses, so any pixel the widget touches stands out.
const HOST: Color = Color::rgb(1.0, 0.0, 1.0);

fn field(text: &str) -> TextField {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    TextField::new(font).with_text(text).with_padding(PADDING)
}

fn in_both_modes(body: impl Fn(bool)) {
    let _profile = crate::input_profile::profile_test_lock();
    crate::device_scale::set_device_scale(1.0);
    for lcd in [true, false] {
        crate::font_settings::set_lcd_enabled(lcd);
        body(lcd);
    }
    crate::font_settings::clear_lcd_enabled_override();
}

/// Lay `f` out at `W`×`H` and paint it over a `host` background.
fn render(f: &mut TextField, host: Color) -> Framebuffer {
    f.set_bounds(Rect::new(0.0, 0.0, W as f64, H as f64));
    f.layout(Size::new(W as f64, H as f64));
    f.set_bounds(Rect::new(0.0, 0.0, W as f64, H as f64));
    let mut fb = Framebuffer::new(W, H);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(host);
        paint_subtree(f, &mut ctx);
    }
    fb
}

/// RGB of the pixel at `(x, y)`, Y-up (row 0 is the bottom).
fn rgb(fb: &Framebuffer, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2]]
}

fn rgb_of(c: Color) -> [u8; 3] {
    let c = c.to_rgba8();
    [c.r, c.g, c.b]
}

fn close(a: [u8; 3], b: [u8; 3], tol: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tol)
}

#[test]
fn rust_only_framed_field_fills_its_background_and_strokes_its_border() {
    in_both_modes(|lcd| {
        let mut f = field("");
        assert!(f.has_frame(), "framed by default");
        let fb = render(&mut f, HOST);
        let bg = rgb_of(crate::theme::current_visuals().widget_bg);
        let center = rgb(&fb, W / 2, H / 2);
        assert!(
            close(center, bg, 2),
            "lcd={lcd}: centre {center:?} should be the widget background {bg:?}"
        );
        let edge = rgb(&fb, W / 2, 0);
        assert!(
            !close(edge, rgb_of(HOST), 1) && !close(edge, bg, 1),
            "lcd={lcd}: bottom edge {edge:?} should carry the border stroke"
        );
    });
}

#[test]
fn rust_only_frameless_focused_field_shows_only_its_text_and_caret() {
    let host = rgb_of(HOST);
    in_both_modes(|lcd| {
        let mut f = field("").with_frame(false);
        assert!(!f.has_frame());
        f.on_event(&Event::FocusGained);
        let fb = render(&mut f, HOST);
        // Nothing but the caret (at the left inset, focused at blink phase 0)
        // may differ from the host: no fill, no border, no focus ring.
        let caret_lo = PADDING as u32 - 2;
        let caret_hi = PADDING as u32 + 2;
        for y in 0..H {
            for x in 0..W {
                if (caret_lo..=caret_hi).contains(&x) {
                    continue;
                }
                let got = rgb(&fb, x, y);
                assert!(
                    close(got, host, 1),
                    "lcd={lcd}: pixel ({x}, {y}) is {got:?}, expected the host's {host:?}"
                );
            }
        }
    });
}

#[test]
fn rust_only_frameless_field_text_sits_where_framed_text_does() {
    in_both_modes(|lcd| {
        let bg = crate::theme::current_visuals().widget_bg;
        let framed = render(&mut field("Hello frame"), bg);
        let frameless = render(&mut field("Hello frame").with_frame(false), bg);
        let mut ink = 0;
        for y in 3..H - 3 {
            for x in 3..W - 3 {
                let (a, b) = (rgb(&framed, x, y), rgb(&frameless, x, y));
                assert!(
                    close(a, b, 3),
                    "lcd={lcd}: pixel ({x}, {y}) framed {a:?} vs frameless {b:?}"
                );
                if !close(a, rgb_of(bg), 8) {
                    ink += 1;
                }
            }
        }
        assert!(ink > 30, "lcd={lcd}: the text should draw ink ({ink} px)");
    });
}
