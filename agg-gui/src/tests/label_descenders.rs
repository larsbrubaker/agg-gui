//! Regression test: a `Label` on the one-em line box ([`LineBox::Em`],
//! agg-sharp's `TextWidget` box) must not cut off the ink of its glyphs.
//!
//! A one-em box is shorter than most faces' ink span (ascent + descent is
//! more than an em), so the text overhangs the box a little above and below.
//! The label used to clip its text to its bounds — and its backbuffer was
//! exactly its bounds — so the bottoms of "g", "j", "p", "q" and "y" were
//! sheared off (MatterCAD's tab titles). The label's glyphs must come out
//! exactly as the same run drawn unclipped at the same baseline, painted
//! directly and through the backbuffer, at 1x and 2x.

use crate::font_settings::LineBox;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::text::{measure_text_metrics, Font};
use crate::widget::{paint_subtree, Widget};
use crate::widgets::Label;
use crate::{Color, Rect, Size};
use std::sync::Arc;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
const TEXT: &str = "gjpqy";
const SIZE: f64 = 16.0;
/// Room around the label so ink outside its box has somewhere to land.
const MARGIN: f64 = 10.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// The (min, max) Y-up rows holding ink in `fb`.
fn ink_rows(fb: &Framebuffer) -> (u32, u32) {
    let (w, h) = (fb.width(), fb.height());
    let px = fb.pixels();
    let (mut lo, mut hi) = (u32::MAX, 0u32);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            if (px[i] as u32 + px[i + 1] as u32 + px[i + 2] as u32) < 3 * 250 {
                lo = lo.min(y);
                hi = hi.max(y);
            }
        }
    }
    assert!(lo <= hi, "nothing was drawn");
    (lo, hi)
}

/// The label, laid out on the one-em box, painted at (MARGIN, MARGIN).
fn label_ink(buffered: bool, scale: f64) -> ((u32, u32), Size) {
    let mut label = Label::new(TEXT, font())
        .with_font_size(SIZE)
        .with_line_box(LineBox::Em)
        .with_color(Color::black())
        .with_has_backbuffer(buffered);
    let used = label.layout(Size::new(400.0, 100.0));
    label.set_bounds(Rect::new(0.0, 0.0, used.width, used.height));
    let (w, h) = canvas(used, scale);
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.scale(scale, scale);
        ctx.translate(MARGIN, MARGIN);
        paint_subtree(&mut label, &mut ctx);
    }
    (ink_rows(&fb), used)
}

/// The same run drawn straight onto the canvas, unclipped, at the baseline the
/// label centres it on.
fn unclipped_ink(used: Size, scale: f64) -> (u32, u32) {
    let font = font();
    let (w, h) = canvas(used, scale);
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.scale(scale, scale);
        ctx.translate(MARGIN, MARGIN);
        ctx.set_font(Arc::clone(&font));
        ctx.set_font_size(SIZE);
        ctx.set_fill_color(Color::black());
        let baseline = measure_text_metrics(&font, TEXT, SIZE).centered_baseline_y(used.height);
        ctx.fill_text(TEXT, 0.0, baseline);
    }
    ink_rows(&fb)
}

fn canvas(used: Size, scale: f64) -> (u32, u32) {
    (
        ((used.width + 2.0 * MARGIN) * scale).ceil() as u32,
        ((used.height + 2.0 * MARGIN) * scale).ceil() as u32,
    )
}

#[test]
fn em_box_label_keeps_its_descenders_and_ascenders() {
    let _profile = crate::input_profile::profile_test_lock();
    crate::font_settings::set_lcd_enabled(false);
    crate::device_scale::set_device_scale(1.0);
    let metrics = measure_text_metrics(&font(), TEXT, SIZE);
    assert!(
        metrics.ascent + metrics.descent > SIZE,
        "the face's ink span overhangs the one-em box, as the case needs"
    );
    for scale in [1.0, 2.0] {
        for buffered in [false, true] {
            let (label, used) = label_ink(buffered, scale);
            let reference = unclipped_ink(used, scale);
            assert_eq!(
                label, reference,
                "buffered={buffered} scale={scale}: the label's ink rows match the \
                 unclipped run's (box {used:?})"
            );
        }
    }
    crate::font_settings::clear_lcd_enabled_override();
}
