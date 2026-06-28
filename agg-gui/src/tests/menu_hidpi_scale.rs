//! Regression test: text rendered INSIDE a CPU-backbuffered widget (the menu
//! bar) must scale with the effective CTM scale (`device × ux`), and must
//! RE-RASTERISE when that scale changes.
//!
//! [`backbuffer_scale.rs`](super::backbuffer_scale) proves the backbuffer
//! *bitmap* follows the CTM, but it only fills a rect and only paints once.
//! On mobile the UX zoom (`ux_scale ≈ 1.7`) is applied *after* the first frame
//! (the platform shell calls `set_client_platform` once it knows the device),
//! so the menu bar's backbuffer is first built at `ux = 1.0` and must rebuild
//! when the zoom arrives.  It didn't — the cache's "needs raster" test missed
//! a pure effective-scale change — so the bar (and its text) stayed at the
//! original size while every GL-FBO widget (windows) scaled correctly: the
//! "menu tiny on mobile" report.  This pins the behaviour so it can't return.

use super::*;

use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::menu::{MenuBar, TopMenu};
use crate::{Color, DrawCtx, Rect, Size};
use std::sync::Arc;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn make_bar() -> MenuBar {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    MenuBar::new(
        Arc::clone(&font),
        vec![TopMenu::new("File", vec![]), TopMenu::new("Edit", vec![])],
        |_| {},
    )
}

/// Lay out `bar` for a viewport whose LOGICAL width is `physical_w / effective`
/// (mirroring `App::layout`'s divide-by-effective-scale), paint it into a
/// physical-pixel framebuffer through a `ctx.scale(effective)` transform
/// (mirroring `App::paint`), and return the count of "ink" pixels — those that
/// differ noticeably from the bar's opaque background fill (i.e. glyph
/// coverage).  Bigger text ⇒ more ink.
fn paint_and_count_ink(bar: &mut MenuBar, physical_w: f64, effective: f64) -> u64 {
    let logical_w = physical_w / effective;
    let used = bar.layout(Size::new(logical_w, 40.0));
    // The host (MenuBarStrip) gives the bar the full available width.
    bar.set_bounds(Rect::new(0.0, 0.0, logical_w.max(used.width), used.height));

    let w = physical_w.ceil() as u32;
    let h = (used.height * effective).ceil() as u32;
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.scale(effective, effective);
        paint_subtree(bar, &mut ctx);
    }

    // The bar fills its bounds with an opaque bg; the dominant colour is that
    // bg.  Sample it from the right side of the bar (past the two buttons,
    // guaranteed empty) and count pixels far from it.
    let px = fb.pixels(); // RGBA8, 4 B/px
    let bg = {
        let sx = (w as f64 * 0.92) as u32;
        let sy = h / 2;
        let i = (sy * w + sx) as usize * 4;
        (px[i] as i32, px[i + 1] as i32, px[i + 2] as i32)
    };
    let mut ink = 0u64;
    for c in px.chunks_exact(4) {
        let d = (c[0] as i32 - bg.0).abs() + (c[1] as i32 - bg.1).abs() + (c[2] as i32 - bg.2).abs();
        if d > 80 {
            ink += 1;
        }
    }
    ink
}

/// The menu bar's backbuffer is built at `ux = 1.0` (first frame) and must
/// rebuild — with text scaled — when the mobile UX zoom arrives.  Uses the
/// SAME bar instance across both paints so the backbuffer cache persists, the
/// way it does in the running app.
#[test]
fn menu_bar_text_rerasters_when_effective_scale_changes() {
    crate::font_settings::set_lcd_enabled(false); // mobile path: grayscale AA
    crate::device_scale::set_device_scale(1.0);

    const PHYS_W: f64 = 400.0;
    let mut bar = make_bar();

    // Frame 1: desktop scale — builds the backbuffer at 1×.
    let ink_1x = paint_and_count_ink(&mut bar, PHYS_W, 1.0);

    // Frame 2: the shell applies the mobile UX zoom.  Same physical canvas,
    // so the logical viewport shrinks and the effective scale doubles.
    let ink_2x = paint_and_count_ink(&mut bar, PHYS_W, 2.0);

    crate::font_settings::clear_lcd_enabled_override();
    crate::device_scale::set_device_scale(1.0);

    assert!(ink_1x > 0, "baseline frame must render menu-bar text (got {ink_1x})");
    // Doubling the effective scale doubles each glyph in both axes ⇒ ~4× ink.
    // A stale backbuffer (the bug) leaves ink_2x ≈ ink_1x.  Require a clear
    // scale signal so the test fails loudly if the text stops scaling.
    assert!(
        ink_2x as f64 > ink_1x as f64 * 2.5,
        "menu-bar text did not re-raster at the new effective scale: \
         ink 1× = {ink_1x}, ink 2× = {ink_2x} (expected ~4×). \
         The CPU backbuffer kept its stale (small) bitmap."
    );
}
