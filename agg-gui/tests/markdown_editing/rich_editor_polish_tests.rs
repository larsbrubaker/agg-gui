//! agg-sharp's `RichEditorPolishTests` (`Tests/Agg.Tests/MarkdigAgg/
//! RichEditorPolishTests.cs`), against `markdown_editing::layout_style`'s
//! colours.  Only `CodeAndStripeShadingShowOnLightAndDarkThemes` is here so
//! far; the class's aligned-HTML parser tests and its two `EndsList` tests
//! (which need `RichDocumentView`) follow with those steps.
//!
//! C# measures the link with `ThemeConfig.ContrastRatio`, the unrounded WCAG
//! ratio of two 8-bit colours; agg-gui has no ThemeConfig, so `wcag_ratio`
//! below is that measuring stick.

use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::markdown_editing::{RichLayoutFonts, RichLayoutStyle, RichThemeColors};
use agg_gui::Color;

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");

fn byte(c: f32) -> i32 {
    (c * 255.0).round() as i32
}

// C# `ThemeConfig.ContrastRatio`: WCAG 2 on 8-bit channels, unrounded.
fn wcag_ratio(a: Color, b: Color) -> f64 {
    fn channel(c: f32) -> f64 {
        let c = byte(c) as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let lum = |c: Color| 0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b);
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[test]
fn code_and_stripe_shading_show_on_light_and_dark_themes() {
    let font = Arc::new(Font::from_slice(REGULAR).expect("demo font"));
    let themes = [
        (Color::from_rgb8(0x33, 0x33, 0x33), Color::white()),
        (
            Color::from_rgb8(0xdd, 0xdd, 0xdd),
            Color::from_rgb8(0x33, 0x33, 0x33),
        ),
    ];
    for (text, background) in themes {
        let theme = RichThemeColors {
            text,
            // C#'s ThemeConfig default accent.
            accent: Color::from_rgb8(0x7A, 0xD7, 0xF0),
            background,
        };
        let style = RichLayoutStyle::with_theme(
            RichLayoutFonts {
                regular: Arc::clone(&font),
                bold: None,
                mono: None,
            },
            Some(&theme),
            Some(1.0),
        );
        for shade in [style.code_background_color(), style.table_stripe_color()] {
            // C# `background.Blend(new Color(shade, 255), shade.Alpha0To255 / 255.0)`.
            let weight = byte(shade.a) as f32 / 255.0;
            let shown = background.lerp(shade.with_alpha(1.0), weight);
            let difference = (byte(shown.r) - byte(background.r)).abs();
            assert!(
                difference >= 10,
                "shading a reader cannot see marks nothing"
            );
        }

        assert!(wcag_ratio(style.link_color, background) >= 4.4);
    }
}
