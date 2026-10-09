//! Pixel tests for [`TextArea::with_frame`] (`frame.rs`): a framed area paints
//! its background and border, a frameless one leaves the host's background
//! showing (no fill, no border, no focus ring) with its text in the same place,
//! and a frameless area scrolls without the over-scan band yet still clips its
//! text to the padded inner rect.
//!
//! Each test paints the production widget through the software `GfxCtx` and
//! `paint_subtree` (the backbuffer path the app uses), under both the LCD and
//! the grayscale backbuffer modes.

use std::sync::Arc;

use super::*;
use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::widget::{paint_subtree, Widget};

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

const W: u32 = 200;
const H: u32 = 100;
const PADDING: f64 = 8.0;

/// A colour no theme uses, so any pixel the widget touches stands out.
const HOST: Color = Color::rgb(1.0, 0.0, 1.0);

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn area(text: &str) -> TextArea {
    TextArea::new(font())
        .with_text(text)
        .with_hint_text("")
        .with_padding(PADDING)
}

/// Run `body` once with LCD backbuffers and once with grayscale ones, holding
/// the profile lock that guards the global typography and scale settings.
fn in_both_modes(body: impl Fn(bool)) {
    let _profile = crate::input_profile::profile_test_lock();
    crate::device_scale::set_device_scale(1.0);
    for lcd in [true, false] {
        crate::font_settings::set_lcd_enabled(lcd);
        body(lcd);
    }
    crate::font_settings::clear_lcd_enabled_override();
}

/// Lay `ta` out at `W`×`H` and paint it over a `host` background.
fn render(ta: &mut TextArea, host: Color) -> Framebuffer {
    ta.layout(Size::new(W as f64, H as f64));
    ta.set_bounds(Rect::new(0.0, 0.0, W as f64, H as f64));
    let mut fb = Framebuffer::new(W, H);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(host);
        paint_subtree(ta, &mut ctx);
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

/// Every pixel for which `keep(x, y)` holds must be the host colour.
fn assert_host_shows(fb: &Framebuffer, what: &str, keep: impl Fn(u32, u32) -> bool) {
    let host = rgb_of(HOST);
    for y in 0..H {
        for x in 0..W {
            if keep(x, y) {
                let got = rgb(fb, x, y);
                assert!(
                    close(got, host, 1),
                    "{what}: pixel ({x}, {y}) is {got:?}, expected the host's {host:?}"
                );
            }
        }
    }
}

#[test]
fn rust_only_framed_area_fills_its_background_and_strokes_its_border() {
    in_both_modes(|lcd| {
        let mut ta = area("");
        assert!(ta.has_frame(), "framed by default");
        let fb = render(&mut ta, HOST);
        let bg = rgb_of(crate::theme::current_visuals().widget_bg);
        let center = rgb(&fb, W / 2, H / 2);
        assert!(
            close(center, bg, 2),
            "lcd={lcd}: centre {center:?} should be the widget background {bg:?}"
        );
        // The border runs along the bottom edge (row 0), mid-width.
        let edge = rgb(&fb, W / 2, 0);
        assert!(
            !close(edge, rgb_of(HOST), 1) && !close(edge, bg, 1),
            "lcd={lcd}: bottom edge {edge:?} should carry the border stroke"
        );
    });
}

#[test]
fn rust_only_frameless_area_leaves_the_host_background_showing() {
    in_both_modes(|lcd| {
        let mut ta = area("").with_frame(false);
        assert!(!ta.has_frame());
        let fb = render(&mut ta, HOST);
        assert_host_shows(&fb, &format!("lcd={lcd} frameless"), |_, _| true);
    });
}

#[test]
fn rust_only_frameless_focused_area_draws_no_focus_ring() {
    // Outside the padded inner rect (with a pixel of slack for the caret's
    // anti-aliased edge) there is nothing but the frame.
    let outside_inner = |x: u32, y: u32| {
        let lo = PADDING as u32 - 1;
        x < lo || y < lo || x >= W - lo || y >= H - lo
    };
    in_both_modes(|lcd| {
        let mut framed = area("Hello");
        framed.on_event(&Event::FocusGained);
        let fb = render(&mut framed, HOST);
        let accent = rgb_of(crate::theme::current_visuals().accent);
        let edge = rgb(&fb, W / 2, 0);
        assert!(
            close(edge, accent, 40),
            "lcd={lcd}: a framed focused area rings in the accent {accent:?}, got {edge:?}"
        );

        let mut frameless = area("Hello").with_frame(false);
        frameless.on_event(&Event::FocusGained);
        let fb = render(&mut frameless, HOST);
        assert_host_shows(&fb, &format!("lcd={lcd} frameless focused"), outside_inner);
    });
}

#[test]
fn rust_only_frameless_text_sits_where_framed_text_does() {
    in_both_modes(|lcd| {
        // Over a host painted in the widget background, the only difference
        // a frameless area may make is the missing border.
        let bg = crate::theme::current_visuals().widget_bg;
        let framed = render(&mut area("Hello frame\nsecond line"), bg);
        let frameless = render(&mut area("Hello frame\nsecond line").with_frame(false), bg);
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
        assert!(ink > 50, "lcd={lcd}: the text should draw ink ({ink} px)");
    });
}

#[test]
fn rust_only_frameless_area_scrolls_without_a_band_and_clips_to_its_padding() {
    let text = (0..40)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    in_both_modes(|lcd| {
        // A framed area of the same content scrolls through the band.
        let mut framed = area(&text);
        framed.layout(Size::new(W as f64, H as f64));
        assert!(
            framed.backbuffer_band().is_some(),
            "lcd={lcd}: framed bands"
        );

        let mut ta = area(&text).with_frame(false);
        render(&mut ta, HOST);
        assert!(
            ta.backbuffer_band().is_none(),
            "lcd={lcd}: frameless never bands"
        );
        let rasters = ta.debug_raster_count();

        // Scroll by a fractional number of lines so a line straddles the top
        // and bottom padding.
        ta.vbar.offset = ta.cached_line_h * 2.5;
        let fb = render(&mut ta, HOST);
        assert_eq!(
            ta.debug_raster_count(),
            rasters + 1,
            "lcd={lcd}: a scroll re-rasters the unbanded cache"
        );
        // The padding rows stay clear (left of the floating scroll bar).
        let lo = PADDING as u32 - 1;
        assert_host_shows(&fb, &format!("lcd={lcd} scrolled padding"), |x, y| {
            x < W - 12 && (y < lo || y >= H - lo)
        });
        // And the scrolled text did paint.
        let host = rgb_of(HOST);
        let inked = (lo..H - lo).any(|y| (lo..W - 12).any(|x| !close(rgb(&fb, x, y), host, 8)));
        assert!(inked, "lcd={lcd}: the scrolled lines should draw");
    });
}

#[test]
fn rust_only_set_frame_rerasters_with_the_new_frame() {
    in_both_modes(|lcd| {
        let mut ta = area("");
        render(&mut ta, HOST);
        ta.set_frame(false);
        let fb = render(&mut ta, HOST);
        assert_host_shows(&fb, &format!("lcd={lcd} after set_frame(false)"), |_, _| {
            true
        });
        ta.set_frame(true);
        let fb = render(&mut ta, HOST);
        let bg = rgb_of(crate::theme::current_visuals().widget_bg);
        assert!(
            close(rgb(&fb, W / 2, H / 2), bg, 2),
            "lcd={lcd}: set_frame(true) paints the background again"
        );
    });
}
