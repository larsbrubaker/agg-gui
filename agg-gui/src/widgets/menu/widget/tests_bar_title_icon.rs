//! agg-sharp `MenuBarTitleIconTests` (Tests/Agg.Tests/Agg.UI/MenuBarTitleIconTests.cs):
//! a [`MenuBar`] title leads with its menu's icon, painted in the title's text
//! colour ahead of the text, and the title widens to hold it.
//!
//! CONVERTED: C# gives `MenuItemModel` an `IconGlyph` that `MenuBarTitle`
//! draws in padding of `IconSize + IconGap` (16 + 4) points. agg-gui puts the
//! Font Awesome glyph in the title text itself (`"\u{F009} Demos"`, drawn
//! through the font's fallback chain), so the title's width comes from its
//! text, which here is two characters longer, not from C#'s 20 points. The
//! test pins what a user sees: the iconed title is wider than the plain one
//! and still holds its whole label, the glyph's ink sits ahead of the text
//! in the title's text colour, and an explicit id keeps C#'s `"Demos Menu"`
//! name free of the glyph.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::framebuffer::Framebuffer;
use crate::geometry::{Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};

use super::super::geometry::{effective_metrics, BAR_H};
use super::tests_conformance::reset_env;
use super::{MenuBar, TopMenu};

/// Font Awesome 4.7 `fa-th-large`, agg-gui's demo "Demos" icon.
const ICON: &str = "\u{F009}";

/// Bar text 9 px in from the title's left (`labels.rs` `BarLabels::paint_in`).
const TEXT_INSET: f64 = 9.0;

fn rgb(fb: &Framebuffer, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * fb.width() + x) * 4) as usize;
    [fb.pixels()[i], fb.pixels()[i + 1], fb.pixels()[i + 2]]
}

#[test]
fn a_title_with_an_icon_glyph_draws_it_ahead_of_the_text() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    // The conformance harness's face, with agg-gui's standard fallback chain
    // (Font Awesome) behind it, as an app's UI font has.
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(crate::fonts::with_standard_fallbacks(
        Font::from_slice(FONT_BYTES).expect("font"),
    ));
    let mut bar = MenuBar::new(
        Arc::clone(&font),
        vec![
            TopMenu::new(format!("{ICON} Demos"), Vec::new()).with_id("Demos Menu"),
            TopMenu::new("Demos", Vec::new()),
        ],
        |_| {},
    );
    let size = Size::new(400.0, BAR_H);
    bar.layout(size);
    bar.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));

    let iconed = bar.children()[0].bounds();
    let plain = bar.children()[1].bounds();
    assert_eq!(bar.children()[0].id(), Some("Demos Menu"));
    assert!(
        iconed.width > plain.width,
        "the icon widens its title ({iconed:?} vs {plain:?})"
    );
    let font_size = effective_metrics().default_font_size;
    let label_w = crate::text::measure_advance(&font, &format!("{ICON} Demos"), font_size);
    assert!(
        TEXT_INSET + label_w <= iconed.width,
        "the title holds its whole label ({label_w} in {iconed:?})"
    );

    // Paint the bar and look for ink in the glyph's own columns, before the
    // text starts.
    let mut fb = Framebuffer::new(size.width as u32, size.height as u32);
    let bg;
    let text;
    {
        let mut ctx = GfxCtx::new(&mut fb);
        bg = ctx.visuals().top_bar_bg;
        text = ctx.visuals().text_color;
        paint_subtree(&mut bar, &mut ctx);
    }
    let to_rgb = |c: Color| {
        [
            (f64::from(c.r) * 255.0).round() as i32,
            (f64::from(c.g) * 255.0).round() as i32,
            (f64::from(c.b) * 255.0).round() as i32,
        ]
    };
    let (bg, text) = (to_rgb(bg), to_rgb(text));
    let glyph_w = crate::text::measure_advance(&font, ICON, font_size);
    assert!(glyph_w > 0.0, "the fallback chain draws the icon glyph");
    let x0 = (iconed.x + TEXT_INSET) as u32;
    let x1 = (iconed.x + TEXT_INSET + glyph_w).ceil() as u32;
    // A pixel is glyph ink when it sits nearer the text colour than the bar
    // fill, per channel sum.
    let dist =
        |a: [i32; 3], b: [u8; 3]| -> i32 { (0..3).map(|i| (a[i] - b[i] as i32).abs()).sum() };
    let mut ink = 0;
    for y in 0..fb.height() {
        for x in x0..x1 {
            let p = rgb(&fb, x, y);
            if dist(text, p) < dist(bg, p) {
                ink += 1;
            }
        }
    }
    assert!(
        ink > 0,
        "the glyph is drawn in the title's text colour ahead of the text"
    );
}
