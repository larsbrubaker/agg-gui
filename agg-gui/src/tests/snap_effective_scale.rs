//! The pixel snaps follow the effective scale (device scale times the Text Size
//! multiplier, `ux_scale`), as C#'s `DeviceScale` does, not the device scale
//! alone; and a baseline snapped under a scaled transform lands on a device
//! pixel row, not a logical one.
//!
//! * A `Label` and a `TextField` in `LineBox::EmSnapped` at ux scale 1.25 are
//!   the boxes `line_box_snapped` works out by hand for device scale 1.25.
//! * A buffered label's software text places its baseline on the row a direct
//!   coverage-mask composite does at 2x: a baseline of 3.5 logical units is
//!   device row 7, not the row of logical 4 (device 8).

use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::ux_scale::set_ux_scale;
use crate::Color;
use std::sync::Arc;

use super::line_box_snapped::{field_box, label_box, liberation_metrics, AT_1X, AT_1_25X};
use crate::font_settings::LineBox;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn at_ux<T>(ux: f64, run: impl FnOnce() -> T) -> T {
    crate::device_scale::set_device_scale(1.0);
    set_ux_scale(ux);
    let result = run();
    set_ux_scale(1.0);
    result
}

#[test]
fn a_label_is_the_snapped_line_box_at_ux_scale_1_25() {
    let font = liberation_metrics();
    at_ux(1.25, || {
        for &(pt, dev_h, dev_base) in &AT_1_25X {
            let (size, y) = label_box(&font, pt, LineBox::EmSnapped);
            assert!(
                (size.height - dev_h / 1.25).abs() < 1e-9,
                "{pt} pt at ux 1.25: height {}, expected {}",
                size.height,
                dev_h / 1.25
            );
            assert!(
                (y - dev_base / 1.25).abs() < 1e-9,
                "{pt} pt at ux 1.25: baseline {y}, expected {}",
                dev_base / 1.25
            );
            let dev_w = size.width * 1.25;
            assert!(
                (dev_w - dev_w.round()).abs() < 1e-9,
                "{pt} pt at ux 1.25: width {} is not whole device pixels",
                size.width
            );
        }
    });
}

#[test]
fn a_text_field_is_the_snapped_line_box_plus_its_insets_at_ux_scale_1_25() {
    let font = liberation_metrics();
    at_ux(1.25, || {
        for &(pt, dev_h, dev_base) in &AT_1_25X {
            // Device scale 1; the ux scale carries the 1.25.
            let (h, y) = field_box(&font, pt, 1.0);
            assert!(
                (h - (dev_h / 1.25 + 6.0)).abs() < 1e-9,
                "{pt} pt at ux 1.25: field height {h}, expected {}",
                dev_h / 1.25 + 6.0
            );
            assert!(
                (y - (3.0 + dev_base / 1.25)).abs() < 1e-9,
                "{pt} pt at ux 1.25: field baseline {y}, expected {}",
                3.0 + dev_base / 1.25
            );
        }
    });
}

/// Device scale and ux scale multiply: 1.25 x 1.25 snaps to 1.5625 device pixels per unit.
#[test]
fn rust_only_device_and_ux_scale_snap_together() {
    let font = liberation_metrics();
    crate::device_scale::set_device_scale(2.0);
    set_ux_scale(1.25);
    let (size, _) = label_box(&font, 10.0, LineBox::EmSnapped);
    set_ux_scale(1.0);
    crate::device_scale::set_device_scale(1.0);
    let device_h = size.height * 2.5;
    assert!(
        (device_h - device_h.round()).abs() < 1e-9,
        "height {} is not whole device pixels at 2 x 1.25",
        size.height
    );
    // And a label at 1x is untouched by the ux-scale changes above.
    assert_eq!(
        label_box(&font, 10.0, LineBox::EmSnapped).0.height,
        AT_1X[3].1
    );
}

/// The lowest row (Y up) with ink after drawing "H" with its baseline at `baseline` logical
/// units under a 2x transform, through the software outline path (`lcd` false, what a
/// buffered label's backbuffer uses) or the coverage-mask composite (`lcd` true, what a direct
/// paint of the GPU renderer uses).
fn lowest_ink_row(baseline: f64, lcd: bool) -> u32 {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut fb = Framebuffer::new(80, 80);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.set_lcd_mode(lcd);
        ctx.scale(2.0, 2.0);
        ctx.set_font(font);
        ctx.set_font_size(12.0);
        ctx.set_fill_color(Color::black());
        ctx.fill_text("H", 4.0, baseline);
    }
    let px = fb.pixels();
    (0..80u32)
        .find(|&y| {
            (0..80u32).any(|x| {
                let i = ((y * 80 + x) * 4) as usize;
                px[i] as u32 + px[i + 1] as u32 + px[i + 2] as u32 > 0
                    && (px[i] as u32 + px[i + 1] as u32 + px[i + 2] as u32) < 3 * 128
            })
        })
        .expect("the text drew ink")
}

#[test]
fn rust_only_a_buffered_baseline_lands_on_the_device_row_at_2x() {
    let _profile = crate::input_profile::profile_test_lock();
    crate::font_settings::set_hinting_enabled(true);
    crate::device_scale::set_device_scale(1.0);
    for baseline in [3.5, 4.0, 4.25, 5.5] {
        let outline = lowest_ink_row(baseline, false);
        // The baseline, in device pixels, rounds to a row: 3.5 -> 7, not 8.
        let expected = (baseline * 2.0 + 0.5).floor() as u32;
        assert_eq!(
            outline, expected,
            "the software baseline at {baseline} logical, 2x: lowest ink row {outline}, expected {expected}"
        );
        let mask = lowest_ink_row(baseline, true);
        assert_eq!(
            outline, mask,
            "buffered (outline) and direct (mask) text share a row at {baseline} logical, 2x"
        );
    }
}
