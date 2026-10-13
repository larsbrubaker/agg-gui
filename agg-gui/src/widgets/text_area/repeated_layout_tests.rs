//! A second `layout` in the same frame must not forget what the first one
//! found. A host that lays a text area out twice before the frame paints (a
//! row that measures its field, then sizes it) used to lose the typed text:
//! the first layout saw the new content signature and invalidated the cached
//! bitmap, the second saw the same signature and, taking the signature as the
//! authority, cleared the dirty flag, so the stale bitmap (the hint, say) was
//! blitted again and the typed text never drew.

use super::theme_tests::{area, close, colors, rgb_of, strongest_pixel};
use super::*;
use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::widget::paint_subtree;

const W: u32 = 200;
const H: u32 = 100;

fn lay_out(ta: &mut TextArea) {
    ta.set_bounds(Rect::new(0.0, 0.0, W as f64, H as f64));
    ta.layout(Size::new(W as f64, H as f64));
    ta.set_bounds(Rect::new(0.0, 0.0, W as f64, H as f64));
}

fn paint(ta: &mut TextArea) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::rgb(1.0, 0.0, 1.0));
    paint_subtree(ta, &mut ctx);
    fb
}

#[test]
fn rust_only_typed_text_draws_when_the_area_is_laid_out_twice_before_the_paint() {
    let _profile = crate::input_profile::profile_test_lock();
    crate::device_scale::set_device_scale(1.0);
    crate::font_settings::set_lcd_enabled(false);
    let theme = colors();
    let (text, background) = (
        theme.text_color_focused.unwrap(),
        theme.background_focused.unwrap(),
    );
    let mut ta = area("", "", theme);
    lay_out(&mut ta);
    ta.on_event(&Event::FocusGained);
    lay_out(&mut ta);
    paint(&mut ta);
    for c in "MMMM".chars() {
        ta.on_event(&Event::KeyDown {
            key: Key::Char(c),
            modifiers: Modifiers::default(),
        });
    }
    // The host's two layouts, then the paint.
    lay_out(&mut ta);
    lay_out(&mut ta);
    let fb = paint(&mut ta);
    let p = strongest_pixel(&fb, rgb_of(background));
    crate::font_settings::clear_lcd_enabled_override();
    assert_eq!(ta.text(), "MMMM");
    assert!(close(p, rgb_of(text)), "the typed text painted {p:?}");
}
