//! Port of agg-sharp `Tests/Agg.Tests/Agg.UI/IconFontTests.cs`: agg-gui's
//! built-in icon font (`crate::icon_font`, the bundled Font Awesome face).
//! Every named constant is a glyph of the face, every one renders ink into
//! its em-box square, and glyphs placed in the em box share one baseline.
//!
//! C# lists the constants by reflection; Rust reads `icon_font::NAMED`.
//! agg-gui bundles Font Awesome 4.7 where agg-sharp embeds 6.7.2 Free Solid;
//! the code points are the same (see `icon_font.rs`).

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::icon_font;

/// Framebuffer row 0 is the bottom, as in agg-sharp's `ImageBuffer`; -1 for
/// an image with no ink.  The pixels are premultiplied, so alpha is the
/// coverage whatever the colour.
fn lowest_ink_row(image: &Framebuffer) -> i32 {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let pixels = image.pixels();
    for y in 0..h {
        for x in 0..w {
            if pixels[(y * w + x) * 4 + 3] > 128 {
                return y as i32;
            }
        }
    }
    -1
}

#[test]
fn every_named_constant_is_a_glyph_in_the_font() {
    let face = icon_font::type_face();
    let missing: Vec<&str> = icon_font::NAMED
        .iter()
        .filter(|(_, glyph)| !face.has_glyph(*glyph))
        .map(|(name, _)| *name)
        .collect();

    assert!(icon_font::NAMED.len() >= 30, "{}", icon_font::NAMED.len());
    assert!(missing.is_empty(), "missing glyphs: {missing:?}");
}

#[test]
fn every_named_glyph_renders_ink_in_its_em_box() {
    for (name, glyph) in icon_font::NAMED {
        let image = icon_font::render(*glyph, Color::black(), 16)
            .unwrap_or_else(|| panic!("{name} rendered no image"));

        assert_eq!(image.width(), 16, "{name}");
        assert_eq!(image.height(), 16, "{name}");
        assert!(lowest_ink_row(&image) >= 0, "{name} has no ink");
    }
}

/// The em box puts every glyph on one baseline at one scale: Font Awesome's B
/// and I both stand on the baseline, so their lowest inked rows match, and the
/// short caret sits above that row rather than being centred in the square.
#[test]
fn glyphs_share_one_baseline_in_the_em_box() {
    let bold = icon_font::render(icon_font::BOLD, Color::black(), 32).expect("bold");
    let italic = icon_font::render(icon_font::ITALIC, Color::black(), 32).expect("italic");

    assert_eq!(lowest_ink_row(&bold), lowest_ink_row(&italic));
}
