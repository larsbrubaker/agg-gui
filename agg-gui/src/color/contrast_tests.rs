//! Tests for `color/contrast.rs` (agg-sharp's `ColorExtensionMethods`
//! contrast helpers).  agg-sharp has no test class for them, so these are
//! `rust_only_*`: the expected colours were computed by stepping C#'s
//! `WithContrast` (8-bit `Color`, float `ColorF` HSL) by hand, and the two
//! C# bugs the port fixes are pinned.

use super::super::Color;

const ACCENT: Color = Color::from_rgb8(0x7A, 0xD7, 0xF0);

#[test]
fn rust_only_contrast_spans_one_to_twenty_one() {
    assert_eq!(Color::black().contrast(Color::white()), 21.0);
    assert_eq!(Color::white().contrast(Color::black()), 21.0);
    assert_eq!(Color::white().contrast(Color::white()), 1.0);
    // #777 on white is the textbook 4.48, rounded to one place.
    assert_eq!(
        Color::from_rgb8(0x77, 0x77, 0x77).contrast(Color::white()),
        4.5
    );
}

#[test]
fn rust_only_luminance_of_black_and_white() {
    assert_eq!(Color::black().luminance(), 0.0);
    assert!((Color::white().luminance() - 1.0).abs() < 1e-12);
}

#[test]
fn rust_only_with_contrast_darkens_on_light_and_keeps_on_dark() {
    // On white the accent darkens 14 steps to reach body-text contrast.
    let on_white = ACCENT.with_contrast(Color::white(), 4.5);
    assert_eq!(on_white, Color::from_rgb8(21, 129, 156));
    assert_eq!(on_white.contrast(Color::white()), 4.5);

    // C#'s default minimum (3) stops sooner.
    assert_eq!(
        ACCENT.with_contrast(Color::white(), 3.0),
        Color::from_rgb8(25, 158, 192)
    );

    // Already readable on a dark background: unchanged.
    let dark = Color::from_rgb8(0x33, 0x33, 0x33);
    assert_eq!(ACCENT.with_contrast(dark, 4.5), ACCENT);
}

#[test]
fn rust_only_with_contrast_lightens_on_dark() {
    let dark_blue = Color::from_rgb8(0x10, 0x20, 0x60);
    let black = Color::from_rgb8(0, 0, 0);
    let adjusted = dark_blue.with_contrast(black, 4.5);
    assert!(adjusted.luminance() > dark_blue.luminance());
    assert!(adjusted.contrast(black) >= 4.5);
}

#[test]
fn rust_only_with_contrast_gives_up_after_thirty_steps() {
    // Black has no lightness to scale, so it never gains contrast on black.
    assert_eq!(
        Color::black().with_contrast(Color::black(), 4.5),
        Color::black()
    );
}

#[test]
fn rust_only_hsl_round_trips() {
    let (h, s, l) = ACCENT.to_hsl();
    let back = Color::from_hsl(h, s, l, 1.0).quantized_to_rgba8();
    assert_eq!(back, ACCENT);
}

#[test]
fn rust_only_overlay_on_composites_translucent_over_under() {
    let half_black = Color::rgba(0.0, 0.0, 0.0, 0.5);
    let shown = half_black.overlay_on(Color::white());
    assert_eq!(shown, Color::rgba(0.5, 0.5, 0.5, 1.0));
    // An opaque colour is returned as is.
    assert_eq!(ACCENT.overlay_on(Color::black()), ACCENT);
}

#[test]
fn rust_only_translucent_contrast_measures_over_black_and_white() {
    // C# bug fixed: OverlayOn mutated the boxed colour, so "over white" was
    // the colour over black again and the result collapsed to the
    // over-black contrast.  The result is the middle of the over-black and
    // over-white contrasts.
    let half_white = Color::rgba(1.0, 1.0, 1.0, 0.5);
    let over_black = half_white
        .overlay_on(Color::black())
        .contrast(Color::black());
    let over_white = half_white
        .overlay_on(Color::white())
        .contrast(Color::black());
    assert_eq!(over_white, 21.0);
    let expected = ((over_black + over_white) / 2.0 * 100.0).round() / 100.0;
    assert_eq!(half_white.contrast(Color::black()), expected);
    assert_ne!(half_white.contrast(Color::black()), over_black);
}

#[test]
fn rust_only_adjust_lightness_keeps_alpha() {
    // C# bug fixed: AdjustLightness rebuilt the colour at alpha 1.
    let translucent = Color::rgba(0.4, 0.6, 0.8, 0.25);
    assert_eq!(translucent.adjust_lightness(0.9).a, 0.25);
}
