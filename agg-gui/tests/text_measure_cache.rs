//! Text measurement through the shape cache returns exactly what it did
//! before the cache was made allocation-free, and widgets that keep their
//! last measurement pick up every change that affects it.
//!
//! `measured_table` measures a fixed set of strings, fonts (including a
//! fallback chain and tabular digits), sizes, letter-spacing intervals and
//! font-size scales through the production entry points (`measure_advance`,
//! `measure_text_metrics`, `shape_glyphs`, `Label` layout and its ink
//! overhang) and renders every value as exact IEEE-754 bits. `EXPECTED` was
//! produced by the same function on the code before the change, so any drift
//! in an advance, a glyph id or offset (what paint draws), or a label's ink
//! bounds fails here.
//!
//! The font-swap tests free each font before making the next, so a cache
//! that recognised a font by its address (which the allocator may hand to
//! the next font) would keep a stale measurement or bitmap: `Label`,
//! `TextField` and `TreeView` rows must all follow every swap.

use std::sync::Arc;

use agg_gui::font_settings;
use agg_gui::text::{measure_advance, measure_text_metrics, shape_glyphs, Font};
use agg_gui::widgets::{Label, NodeIcon, TextField, TreeView};
use agg_gui::{Rect, Size, Widget};

const CASCADIA: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
const FA: &[u8] = include_bytes!("../../demo/assets/fa.ttf");
const NOTO: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");

const TEXTS: &[&str] = &[
    "",
    "Hello",
    "Row 12: Width 15.00 mm",
    "0123456789",
    "AV Wa fi ffl Ty",
    "  spaced  out  ",
    "\u{F109} laptop",
    "\u{65E5}\u{672C}",
];
const SIZES: &[f64] = &[9.0, 14.0, 17.5, 28.0, 40.0 / 3.0];
const INTERVALS: &[f64] = &[0.0, 0.05];
const SCALES: &[f64] = &[1.0, 1.5];

fn fonts() -> Vec<(&'static str, Arc<Font>)> {
    let fa = Arc::new(Font::from_slice(FA).expect("fa"));
    vec![
        (
            "cascadia+fa",
            Arc::new(
                Font::from_slice(CASCADIA)
                    .expect("cascadia")
                    .with_fallback(fa),
            ),
        ),
        ("noto", Arc::new(Font::from_slice(NOTO).expect("noto"))),
        (
            "noto-tnum",
            Arc::new(
                Font::from_slice(NOTO)
                    .expect("noto")
                    .with_tabular_digits(true),
            ),
        ),
    ]
}

/// FNV-1a over 64-bit words: a stable digest of everything paint uses from
/// the shaped glyphs (ids, advances, offsets, which font drew each).
fn digest(words: impl IntoIterator<Item = u64>) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for w in words {
        for b in w.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// One line per measured case; every number as exact bits.
fn measured_table() -> Vec<String> {
    let mut out = Vec::new();
    for (name, font) in fonts() {
        for &text in TEXTS {
            for &size in SIZES {
                let glyphs = shape_glyphs(&font, text, size);
                let shape = digest(glyphs.iter().flat_map(|g| {
                    [
                        g.glyph_id as u64,
                        g.x_advance.to_bits(),
                        g.x_offset.to_bits(),
                        g.y_offset.to_bits(),
                        g.fallback_font.is_some() as u64,
                    ]
                }));
                for &interval in INTERVALS {
                    font_settings::set_interval(interval);
                    let advance = measure_advance(&font, text, size);
                    let m = measure_text_metrics(&font, text, size);
                    out.push(format!(
                        "{name} {text:?} {size} {interval}: shape {shape:016x} advance {:016x} \
                         metrics {:016x} {:016x} {:016x} {:016x}",
                        advance.to_bits(),
                        m.width.to_bits(),
                        m.ascent.to_bits(),
                        m.descent.to_bits(),
                        m.line_height.to_bits(),
                    ));
                }
                font_settings::set_interval(0.0);
            }
            for &scale in SCALES {
                font_settings::set_font_size_scale(scale);
                let mut label = Label::new(text, Arc::clone(&font));
                let size = label.layout(Size::new(1000.0, 100.0));
                label.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
                let ink = label.backbuffer_ink_outset().unwrap_or((0.0, 0.0));
                out.push(format!(
                    "{name} {text:?} label x{scale}: size {:016x} {:016x} ink {:016x} {:016x}",
                    size.width.to_bits(),
                    size.height.to_bits(),
                    ink.0.to_bits(),
                    ink.1.to_bits(),
                ));
            }
            font_settings::set_font_size_scale(1.0);
        }
    }
    out
}

#[test]
fn measured_values_are_unchanged() {
    let actual = measured_table();
    let expected: Vec<&str> = EXPECTED.lines().collect();
    if actual != expected {
        for line in &actual {
            println!("{line}");
        }
    }
    assert_eq!(actual.len(), expected.len(), "case count");
    for (a, e) in actual.iter().zip(&expected) {
        assert_eq!(a, e);
    }
}

/// A cache hit returns exactly what the miss that filled it returned, and
/// `measure_advance` agrees with the glyphs `shape_glyphs` hands paint.
#[test]
fn warm_measurement_matches_cold() {
    for (_, font) in fonts() {
        for &text in TEXTS {
            // A size no other test uses, so the first call is a miss.
            let size = 21.25;
            let cold = measure_advance(&font, text, size);
            let warm = measure_advance(&font, text, size);
            assert_eq!(cold.to_bits(), warm.to_bits(), "{text:?}");
            let summed: f64 = shape_glyphs(&font, text, size)
                .iter()
                .map(|g| g.x_advance)
                .sum();
            assert_eq!(cold.to_bits(), summed.to_bits(), "{text:?}");
        }
    }
}

/// `Label` keeps its last measurement across layouts; changing its text,
/// font size, the system font-size scale or the system font must each be
/// picked up, and changing back must give the original width again.
#[test]
fn label_remeasures_when_text_font_or_scale_changes() {
    let fonts = fonts();
    let font = Arc::clone(&fonts[1].1);
    let other_font = Arc::clone(&fonts[0].1);
    let avail = Size::new(1000.0, 100.0);
    let mut label = Label::new("Width", Arc::clone(&font));
    let base = label.layout(avail).width;
    assert_eq!(
        label.layout(avail).width,
        base,
        "unchanged text keeps its width"
    );
    assert_eq!(base, measure_advance(&font, "Width", 14.0));

    label.set_text("Width and height");
    let longer = label.layout(avail).width;
    assert_eq!(longer, measure_advance(&font, "Width and height", 14.0));
    label.set_text("Width");
    assert_eq!(label.layout(avail).width, base, "text change back");

    label.set_font_size(28.0);
    assert_eq!(
        label.layout(avail).width,
        measure_advance(&font, "Width", 28.0)
    );
    label.set_font_size(14.0);
    assert_eq!(label.layout(avail).width, base, "font size back");

    font_settings::set_font_size_scale(1.5);
    let scaled = label.layout(avail).width;
    font_settings::set_font_size_scale(1.0);
    assert_eq!(scaled, measure_advance(&font, "Width", 21.0));
    assert_eq!(label.layout(avail).width, base, "scale back");

    font_settings::set_system_font(Some(Arc::clone(&other_font)));
    let swapped = label.layout(avail).width;
    font_settings::set_system_font(None);
    assert_eq!(swapped, measure_advance(&other_font, "Width", 14.0));
    assert_ne!(swapped, base, "the fixture fonts measure differently");
    assert_eq!(label.layout(avail).width, base, "system font back");
}

/// `measured_table` on the code before the shape cache change.
const EXPECTED: &str = r#"cascadia+fa "" 9 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "" 9 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "" 14 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "" 14 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "" 17.5 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "" 17.5 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "" 28 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "" 28 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "" 13.333333333333334 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "" 13.333333333333334 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "" label x1: size 8000000000000000 4035000000000000 ink 0000000000000000 0000000000000000
cascadia+fa "" label x1.5: size 8000000000000000 403f800000000000 ink 0000000000000000 0000000000000000
cascadia+fa "Hello" 9 0: shape 3d12e41c226b8a26 advance 403a5e0000000000 metrics 403a5e0000000000 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "Hello" 9 0.05: shape 3d12e41c226b8a26 advance 403c9e0000000000 metrics 403c9e0000000000 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "Hello" 14 0: shape 25488261a9c30763 advance 4044820000000000 metrics 4044820000000000 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "Hello" 14 0.05: shape 25488261a9c30763 advance 4046420000000000 metrics 4046420000000000 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "Hello" 17.5 0: shape 3c4ab342494dc661 advance 4049a28000000000 metrics 4049a28000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "Hello" 17.5 0.05: shape 3c4ab342494dc661 advance 404bd28000000000 metrics 404bd28000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "Hello" 28 0: shape e45bf16a6ade8713 advance 4054820000000000 metrics 4054820000000000 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "Hello" 28 0.05: shape e45bf16a6ade8713 advance 4056420000000000 metrics 4056420000000000 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "Hello" 13.333333333333334 0: shape 15184508479dbddc advance 4043880000000000 metrics 4043880000000000 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "Hello" 13.333333333333334 0.05: shape 15184508479dbddc advance 404532aaaaaaaaaa metrics 404532aaaaaaaaaa 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "Hello" label x1: size 4044820000000000 4035000000000000 ink 0000000000000000 0000000000000000
cascadia+fa "Hello" label x1.5: size 404ec30000000000 403f800000000000 ink 0000000000000000 0000000000000000
cascadia+fa "Row 12: Width 15.00 mm" 9 0: shape d58fd27e2df5d4fa advance 405d010000000000 metrics 405d010000000000 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "Row 12: Width 15.00 mm" 9 0.05: shape d58fd27e2df5d4fa advance 405f7a999999999d metrics 405f7a999999999d 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "Row 12: Width 15.00 mm" 14 0: shape cd4dba0a0dbf9c6e advance 40668f0000000000 metrics 40668f0000000000 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "Row 12: Width 15.00 mm" 14 0.05: shape cd4dba0a0dbf9c6e advance 40687bcccccccccb metrics 40687bcccccccccb 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "Row 12: Width 15.00 mm" 17.5 0: shape 35fd7f25464aea56 advance 406c32c000000000 metrics 406c32c000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "Row 12: Width 15.00 mm" 17.5 0.05: shape 35fd7f25464aea56 advance 406e9ac000000000 metrics 406e9ac000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "Row 12: Width 15.00 mm" 28 0: shape ac9d0f54e2c950ce advance 40768f0000000000 metrics 40768f0000000000 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "Row 12: Width 15.00 mm" 28 0.05: shape ac9d0f54e2c950ce advance 40787bcccccccccb metrics 40787bcccccccccb 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "Row 12: Width 15.00 mm" 13.333333333333334 0: shape 88b72684ea030b4a advance 40657c0000000000 metrics 40657c0000000000 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "Row 12: Width 15.00 mm" 13.333333333333334 0.05: shape 88b72684ea030b4a advance 4067515555555554 metrics 4067515555555554 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "Row 12: Width 15.00 mm" label x1: size 40668f0000000000 4035000000000000 ink 0000000000000000 0000000000000000
cascadia+fa "Row 12: Width 15.00 mm" label x1.5: size 4070eb4000000000 403f800000000000 ink 0000000000000000 0000000000000000
cascadia+fa "0123456789" 9 0: shape 3915f286c4ec598e advance 404a5e0000000000 metrics 404a5e0000000000 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "0123456789" 9 0.05: shape 3915f286c4ec598e advance 404c9e0000000002 metrics 404c9e0000000002 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "0123456789" 14 0: shape 36403bc9a2adb1ea advance 4054820000000000 metrics 4054820000000000 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "0123456789" 14 0.05: shape 36403bc9a2adb1ea advance 4056420000000001 metrics 4056420000000001 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "0123456789" 17.5 0: shape 147ee1f3f8e90c6a advance 4059a28000000000 metrics 4059a28000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "0123456789" 17.5 0.05: shape 147ee1f3f8e90c6a advance 405bd28000000000 metrics 405bd28000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "0123456789" 28 0: shape b762ee3aa21744aa advance 4064820000000000 metrics 4064820000000000 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "0123456789" 28 0.05: shape b762ee3aa21744aa advance 4066420000000001 metrics 4066420000000001 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "0123456789" 13.333333333333334 0: shape d921409451d8361e advance 4053880000000000 metrics 4053880000000000 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "0123456789" 13.333333333333334 0.05: shape d921409451d8361e advance 405532aaaaaaaaab metrics 405532aaaaaaaaab 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "0123456789" label x1: size 4054820000000000 4035000000000000 ink 0000000000000000 0000000000000000
cascadia+fa "0123456789" label x1.5: size 405ec30000000000 403f800000000000 ink 0000000000000000 0000000000000000
cascadia+fa "AV Wa fi ffl Ty" 9 0: shape 03a593f7d5aab941 advance 4053c68000000000 metrics 4053c68000000000 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "AV Wa fi ffl Ty" 9 0.05: shape 03a593f7d5aab941 advance 4055768000000002 metrics 4055768000000002 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "AV Wa fi ffl Ty" 14 0: shape 564c182db57a4030 advance 405ec30000000000 metrics 405ec30000000000 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "AV Wa fi ffl Ty" 14 0.05: shape 564c182db57a4030 advance 4060b18000000001 metrics 4060b18000000001 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "AV Wa fi ffl Ty" 17.5 0: shape 12ab82e8bd00e976 advance 406339e000000000 metrics 406339e000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "AV Wa fi ffl Ty" 17.5 0.05: shape 12ab82e8bd00e976 advance 4064dde000000000 metrics 4064dde000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "AV Wa fi ffl Ty" 28 0: shape ade69f2dafebbf20 advance 406ec30000000000 metrics 406ec30000000000 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "AV Wa fi ffl Ty" 28 0.05: shape ade69f2dafebbf20 advance 4070b18000000001 metrics 4070b18000000001 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "AV Wa fi ffl Ty" 13.333333333333334 0: shape d66f6ff0b9588f3b advance 405d4c0000000000 metrics 405d4c0000000000 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "AV Wa fi ffl Ty" 13.333333333333334 0.05: shape d66f6ff0b9588f3b advance 405fcc0000000002 metrics 405fcc0000000002 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "AV Wa fi ffl Ty" label x1: size 405ec30000000000 4035000000000000 ink 0000000000000000 0000000000000000
cascadia+fa "AV Wa fi ffl Ty" label x1.5: size 4067124000000000 403f800000000000 ink 0000000000000000 0000000000000000
cascadia+fa "  spaced  out  " 9 0: shape fdac683a49245b70 advance 4053c68000000000 metrics 4053c68000000000 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "  spaced  out  " 9 0.05: shape fdac683a49245b70 advance 4055768000000002 metrics 4055768000000002 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "  spaced  out  " 14 0: shape a5122f71f68e31a1 advance 405ec30000000000 metrics 405ec30000000000 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "  spaced  out  " 14 0.05: shape a5122f71f68e31a1 advance 4060b18000000001 metrics 4060b18000000001 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "  spaced  out  " 17.5 0: shape c8c8216be41d56c7 advance 406339e000000000 metrics 406339e000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "  spaced  out  " 17.5 0.05: shape c8c8216be41d56c7 advance 4064dde000000000 metrics 4064dde000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "  spaced  out  " 28 0: shape 3d16195b541f3911 advance 406ec30000000000 metrics 406ec30000000000 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "  spaced  out  " 28 0.05: shape 3d16195b541f3911 advance 4070b18000000001 metrics 4070b18000000001 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "  spaced  out  " 13.333333333333334 0: shape 8d171630597c711a advance 405d4c0000000000 metrics 405d4c0000000000 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "  spaced  out  " 13.333333333333334 0.05: shape 8d171630597c711a advance 405fcc0000000002 metrics 405fcc0000000002 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "  spaced  out  " label x1: size 405ec30000000000 4035000000000000 ink 0000000000000000 0000000000000000
cascadia+fa "  spaced  out  " label x1.5: size 4067124000000000 403f800000000000 ink 0000000000000000 0000000000000000
cascadia+fa "\u{f109} laptop" 9 0: shape 625f29f25c7f7de7 advance 4047474924924925 metrics 4047474924924925 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "\u{f109} laptop" 9 0.05: shape 625f29f25c7f7de7 advance 40491415f15f15f3 metrics 40491415f15f15f3 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "\u{f109} laptop" 14 0: shape 011cd4ed52cca878 advance 40521b0000000000 metrics 40521b0000000000 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "\u{f109} laptop" 14 0.05: shape 011cd4ed52cca878 advance 4053816666666667 metrics 4053816666666667 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "\u{f109} laptop" 17.5 0: shape bbcd64e6018b1126 advance 4056a1c000000000 metrics 4056a1c000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "\u{f109} laptop" 17.5 0.05: shape bbcd64e6018b1126 advance 405861c000000000 metrics 405861c000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "\u{f109} laptop" 28 0: shape 251c92ccbdf64a18 advance 40621b0000000000 metrics 40621b0000000000 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "\u{f109} laptop" 28 0.05: shape 251c92ccbdf64a18 advance 4063816666666667 metrics 4063816666666667 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "\u{f109} laptop" 13.333333333333334 0: shape 953414a82702e0c2 advance 40513e4924924924 metrics 40513e4924924924 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "\u{f109} laptop" 13.333333333333334 0.05: shape 953414a82702e0c2 advance 4052939e79e79e7a metrics 4052939e79e79e7a 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "\u{f109} laptop" label x1: size 40521b0000000000 4035000000000000 ink 0000000000000000 0000000000000000
cascadia+fa "\u{f109} laptop" label x1.5: size 405b288000000000 403f800000000000 ink 0000000000000000 0000000000000000
cascadia+fa "日本" 9 0: shape f5012ad36db949a5 advance 4025180000000000 metrics 4025180000000000 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "日本" 9 0.05: shape f5012ad36db949a5 advance 4026e4cccccccccd metrics 4026e4cccccccccd 4020b30000000000 4000e00000000000 4024eb0000000000
cascadia+fa "日本" 14 0: shape 1266cc54288037e5 advance 4030680000000000 metrics 4030680000000000 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "日本" 14 0.05: shape 1266cc54288037e5 advance 4031ce6666666666 metrics 4031ce6666666666 4029fa0000000000 400a400000000000 4030450000000000
cascadia+fa "日本" 17.5 0: shape cfefffa43a0a1025 advance 4034820000000000 metrics 4034820000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "日本" 17.5 0.05: shape cfefffa43a0a1025 advance 4036420000000000 metrics 4036420000000000 40303c4000000000 4010680000000000 4034564000000000
cascadia+fa "日本" 28 0: shape 306d2f86f479c785 advance 4040680000000000 metrics 4040680000000000 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "日本" 28 0.05: shape 306d2f86f479c785 advance 4041ce6666666666 metrics 4041ce6666666666 4039fa0000000000 401a400000000000 4040450000000000
cascadia+fa "日本" 13.333333333333334 0: shape 4a00443673b39385 advance 402f400000000000 metrics 402f400000000000 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "日本" 13.333333333333334 0.05: shape 4a00443673b39385 advance 4030f55555555555 metrics 4030f55555555555 4028bd5555555556 4009000000000000 402efd5555555556
cascadia+fa "日本" label x1: size 4030680000000000 4035000000000000 ink 0000000000000000 0000000000000000
cascadia+fa "日本" label x1.5: size 40389c0000000000 403f800000000000 ink 0000000000000000 0000000000000000
noto "" 9 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "" 9 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "" 14 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "" 14 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "" 17.5 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "" 17.5 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "" 28 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "" 28 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "" 13.333333333333334 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "" 13.333333333333334 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "" label x1: size 8000000000000000 4035000000000000 ink 0000000000000000 0000000000000000
noto "" label x1.5: size 8000000000000000 403f800000000000 ink 0000000000000000 0000000000000000
noto "Hello" 9 0: shape f89476e9339a1fa3 advance 4035d5810624dd2f metrics 4035d5810624dd2f 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "Hello" 9 0.05: shape f89476e9339a1fa3 advance 403815810624dd2f metrics 403815810624dd2f 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "Hello" 14 0: shape 797b343bf2c02269 advance 4040fb645a1cac08 metrics 4040fb645a1cac08 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "Hello" 14 0.05: shape 797b343bf2c02269 advance 4042bb645a1cac09 metrics 4042bb645a1cac09 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "Hello" 17.5 0: shape 211349fc28e339d9 advance 40453a3d70a3d70b metrics 40453a3d70a3d70b 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "Hello" 17.5 0.05: shape 211349fc28e339d9 advance 40476a3d70a3d70b metrics 40476a3d70a3d70b 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "Hello" 28 0: shape f34f1667a5d66cf9 advance 4050fb645a1cac08 metrics 4050fb645a1cac08 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "Hello" 28 0.05: shape f34f1667a5d66cf9 advance 4052bb645a1cac09 metrics 4052bb645a1cac09 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "Hello" 13.333333333333334 0: shape d4d4b0164f5ef555 advance 40402c5f92c5f92d metrics 40402c5f92c5f92d 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "Hello" 13.333333333333334 0.05: shape d4d4b0164f5ef555 advance 4041d70a3d70a3d8 metrics 4041d70a3d70a3d8 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "Hello" label x1: size 4040fb645a1cac08 4035000000000000 ink 0000000000000000 0000000000000000
noto "Hello" label x1.5: size 40497916872b020c 403f800000000000 ink 0000000000000000 0000000000000000
noto "Row 12: Width 15.00 mm" 9 0: shape e76add623f02c929 advance 405a3820c49ba5e2 metrics 405a3820c49ba5e2 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "Row 12: Width 15.00 mm" 9 0.05: shape e76add623f02c929 advance 405cb1ba5e353f7c metrics 405cb1ba5e353f7c 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "Row 12: Width 15.00 mm" 14 0: shape 59a25986e0740845 advance 4064648b43958106 metrics 4064648b43958106 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "Row 12: Width 15.00 mm" 14 0.05: shape 59a25986e0740845 advance 4066515810624dd3 metrics 4066515810624dd3 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "Row 12: Width 15.00 mm" 17.5 0: shape 11ee959abf1289b1 advance 40697dae147ae148 metrics 40697dae147ae148 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "Row 12: Width 15.00 mm" 17.5 0.05: shape 11ee959abf1289b1 advance 406be5ae147ae149 metrics 406be5ae147ae149 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "Row 12: Width 15.00 mm" 28 0: shape 3dc22c07613adb05 advance 4074648b43958106 metrics 4074648b43958106 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "Row 12: Width 15.00 mm" 28 0.05: shape 3dc22c07613adb05 advance 4076515810624dd3 metrics 4076515810624dd3 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "Row 12: Width 15.00 mm" 13.333333333333334 0: shape 2301b60972c7bdc5 advance 40636bf258bf258c metrics 40636bf258bf258c 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "Row 12: Width 15.00 mm" 13.333333333333334 0.05: shape 2301b60972c7bdc5 advance 40654147ae147ae1 metrics 40654147ae147ae1 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "Row 12: Width 15.00 mm" label x1: size 4064648b43958106 4035000000000000 ink 0000000000000000 0000000000000000
noto "Row 12: Width 15.00 mm" label x1.5: size 406e96d0e5604189 403f800000000000 ink 0000000000000000 0000000000000000
noto "0123456789" 9 0: shape e40fc5c10948f886 advance 4049bd70a3d70a3c metrics 4049bd70a3d70a3c 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "0123456789" 9 0.05: shape e40fc5c10948f886 advance 404bfd70a3d70a3d metrics 404bfd70a3d70a3d 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "0123456789" 14 0: shape 8401f7f13df0a8b6 advance 4054051eb851eb85 metrics 4054051eb851eb85 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "0123456789" 14 0.05: shape 8401f7f13df0a8b6 advance 4055c51eb851eb85 metrics 4055c51eb851eb85 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "0123456789" 17.5 0: shape eb019417e2f80676 advance 4059066666666669 metrics 4059066666666669 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "0123456789" 17.5 0.05: shape eb019417e2f80676 advance 405b366666666669 metrics 405b366666666669 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "0123456789" 28 0: shape aa62598ddcfcc136 advance 4064051eb851eb85 metrics 4064051eb851eb85 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "0123456789" 28 0.05: shape aa62598ddcfcc136 advance 4065c51eb851eb85 metrics 4065c51eb851eb85 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "0123456789" 13.333333333333334 0: shape e4a405391bd4d066 advance 4053111111111111 metrics 4053111111111111 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "0123456789" 13.333333333333334 0.05: shape e4a405391bd4d066 advance 4054bbbbbbbbbbbd metrics 4054bbbbbbbbbbbd 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "0123456789" label x1: size 4054051eb851eb85 4035000000000000 ink 0000000000000000 0000000000000000
noto "0123456789" label x1.5: size 405e07ae147ae148 403f800000000000 ink 0000000000000000 0000000000000000
noto "AV Wa fi ffl Ty" 9 0: shape e0ef06227a981ebd advance 404c5e353f7ced92 metrics 404c5e353f7ced92 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "AV Wa fi ffl Ty" 9 0.05: shape e0ef06227a981ebd advance 404fbe353f7ced90 metrics 404fbe353f7ced90 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "AV Wa fi ffl Ty" 14 0: shape fe59b05a177a5237 advance 405610624dd2f1ab metrics 405610624dd2f1ab 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "AV Wa fi ffl Ty" 14 0.05: shape fe59b05a177a5237 advance 4058b0624dd2f1aa metrics 4058b0624dd2f1aa 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "AV Wa fi ffl Ty" 17.5 0: shape 8458495ce0893408 advance 405b947ae147ae14 metrics 405b947ae147ae14 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "AV Wa fi ffl Ty" 17.5 0.05: shape 8458495ce0893408 advance 405edc7ae147ae13 metrics 405edc7ae147ae13 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "AV Wa fi ffl Ty" 28 0: shape 80d28648a39edc87 advance 406610624dd2f1ab metrics 406610624dd2f1ab 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "AV Wa fi ffl Ty" 28 0.05: shape 80d28648a39edc87 advance 4068b0624dd2f1aa metrics 4068b0624dd2f1aa 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "AV Wa fi ffl Ty" 13.333333333333334 0: shape b608bf9f1807f57a advance 40550369d0369d03 metrics 40550369d0369d03 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "AV Wa fi ffl Ty" 13.333333333333334 0.05: shape b608bf9f1807f57a advance 40578369d0369d04 metrics 40578369d0369d04 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "AV Wa fi ffl Ty" label x1: size 405610624dd2f1ab 4035000000000000 ink 0000000000000000 0000000000000000
noto "AV Wa fi ffl Ty" label x1.5: size 40608c49ba5e3540 403f800000000000 ink 0000000000000000 0000000000000000
noto "  spaced  out  " 9 0: shape 0e5fd75cad225f98 advance 404d0f9db22d0e58 metrics 404d0f9db22d0e58 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "  spaced  out  " 9 0.05: shape 0e5fd75cad225f98 advance 405037ced916872a metrics 405037ced916872a 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "  spaced  out  " 14 0: shape ad18aae5327855cc advance 40569a5e353f7cee metrics 40569a5e353f7cee 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "  spaced  out  " 14 0.05: shape ad18aae5327855cc advance 40593a5e353f7cef metrics 40593a5e353f7cef 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "  spaced  out  " 17.5 0: shape 913caf269dad940d advance 405c40f5c28f5c29 metrics 405c40f5c28f5c29 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "  spaced  out  " 17.5 0.05: shape 913caf269dad940d advance 405f88f5c28f5c29 metrics 405f88f5c28f5c29 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "  spaced  out  " 28 0: shape 7aba78e9628d0adc advance 40669a5e353f7cee metrics 40669a5e353f7cee 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "  spaced  out  " 28 0.05: shape 7aba78e9628d0adc advance 40693a5e353f7cef metrics 40693a5e353f7cef 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "  spaced  out  " 13.333333333333334 0: shape 73d3ee649e29370e advance 405586d3a06d3a08 metrics 405586d3a06d3a08 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "  spaced  out  " 13.333333333333334 0.05: shape 73d3ee649e29370e advance 405806d3a06d3a09 metrics 405806d3a06d3a09 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "  spaced  out  " label x1: size 40569a5e353f7cee 4035000000000000 ink 0000000000000000 0000000000000000
noto "  spaced  out  " label x1.5: size 4060f3c6a7ef9db2 403f800000000000 ink 0000000000000000 0000000000000000
noto "\u{f109} laptop" 9 0: shape 7c0078d8ac93e234 advance 40416fffffffffff metrics 40416fffffffffff 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "\u{f109} laptop" 9 0.05: shape 7c0078d8ac93e234 advance 40433ccccccccccc metrics 40433ccccccccccc 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "\u{f109} laptop" 14 0: shape d23443460a538f74 advance 404b200000000000 metrics 404b200000000000 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "\u{f109} laptop" 14 0.05: shape d23443460a538f74 advance 404deccccccccccc metrics 404deccccccccccc 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "\u{f109} laptop" 17.5 0: shape cc3393bc0dc7cfb7 advance 4050f40000000001 metrics 4050f40000000001 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "\u{f109} laptop" 17.5 0.05: shape cc3393bc0dc7cfb7 advance 4052b40000000001 metrics 4052b40000000001 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "\u{f109} laptop" 28 0: shape 78a3d3b04d3fbe74 advance 405b200000000000 metrics 405b200000000000 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "\u{f109} laptop" 28 0.05: shape 78a3d3b04d3fbe74 advance 405deccccccccccc metrics 405deccccccccccc 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "\u{f109} laptop" 13.333333333333334 0: shape 49425fee04a8b392 advance 4049d55555555556 metrics 4049d55555555556 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "\u{f109} laptop" 13.333333333333334 0.05: shape 49425fee04a8b392 advance 404c800000000001 metrics 404c800000000001 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "\u{f109} laptop" label x1: size 404b200000000000 4035000000000000 ink 0000000000000000 0000000000000000
noto "\u{f109} laptop" label x1.5: size 4054580000000001 403f800000000000 ink 0000000000000000 0000000000000000
noto "日本" 9 0: shape 15eea19a2e409e5d advance 4025999999999999 metrics 4025999999999999 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "日本" 9 0.05: shape 15eea19a2e409e5d advance 4027666666666666 metrics 4027666666666666 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto "日本" 14 0: shape c5e3a2e2e9020815 advance 4030cccccccccccd metrics 4030cccccccccccd 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "日本" 14 0.05: shape c5e3a2e2e9020815 advance 4032333333333333 metrics 4032333333333333 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto "日本" 17.5 0: shape 3fe23b354f064335 advance 4035000000000001 metrics 4035000000000001 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "日本" 17.5 0.05: shape 3fe23b354f064335 advance 4036c00000000001 metrics 4036c00000000001 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto "日本" 28 0: shape 2b04ed1e9f4f0995 advance 4040cccccccccccd metrics 4040cccccccccccd 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "日本" 28 0.05: shape 2b04ed1e9f4f0995 advance 4042333333333333 metrics 4042333333333333 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto "日本" 13.333333333333334 0: shape 9743f2b4a368be25 advance 4030000000000000 metrics 4030000000000000 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "日本" 13.333333333333334 0.05: shape 9743f2b4a368be25 advance 4031555555555555 metrics 4031555555555555 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto "日本" label x1: size 4030cccccccccccd 4035000000000000 ink 0000000000000000 0000000000000000
noto "日本" label x1.5: size 4039333333333334 403f800000000000 ink 0000000000000000 0000000000000000
noto-tnum "" 9 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "" 9 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "" 14 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "" 14 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "" 17.5 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "" 17.5 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "" 28 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "" 28 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "" 13.333333333333334 0: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "" 13.333333333333334 0.05: shape cbf29ce484222325 advance 8000000000000000 metrics 8000000000000000 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "" label x1: size 8000000000000000 4035000000000000 ink 0000000000000000 0000000000000000
noto-tnum "" label x1.5: size 8000000000000000 403f800000000000 ink 0000000000000000 0000000000000000
noto-tnum "Hello" 9 0: shape f89476e9339a1fa3 advance 4035d5810624dd2f metrics 4035d5810624dd2f 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "Hello" 9 0.05: shape f89476e9339a1fa3 advance 403815810624dd2f metrics 403815810624dd2f 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "Hello" 14 0: shape 797b343bf2c02269 advance 4040fb645a1cac08 metrics 4040fb645a1cac08 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "Hello" 14 0.05: shape 797b343bf2c02269 advance 4042bb645a1cac09 metrics 4042bb645a1cac09 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "Hello" 17.5 0: shape 211349fc28e339d9 advance 40453a3d70a3d70b metrics 40453a3d70a3d70b 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "Hello" 17.5 0.05: shape 211349fc28e339d9 advance 40476a3d70a3d70b metrics 40476a3d70a3d70b 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "Hello" 28 0: shape f34f1667a5d66cf9 advance 4050fb645a1cac08 metrics 4050fb645a1cac08 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "Hello" 28 0.05: shape f34f1667a5d66cf9 advance 4052bb645a1cac09 metrics 4052bb645a1cac09 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "Hello" 13.333333333333334 0: shape d4d4b0164f5ef555 advance 40402c5f92c5f92d metrics 40402c5f92c5f92d 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "Hello" 13.333333333333334 0.05: shape d4d4b0164f5ef555 advance 4041d70a3d70a3d8 metrics 4041d70a3d70a3d8 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "Hello" label x1: size 4040fb645a1cac08 4035000000000000 ink 0000000000000000 0000000000000000
noto-tnum "Hello" label x1.5: size 40497916872b020c 403f800000000000 ink 0000000000000000 0000000000000000
noto-tnum "Row 12: Width 15.00 mm" 9 0: shape e76add623f02c929 advance 405a3820c49ba5e2 metrics 405a3820c49ba5e2 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "Row 12: Width 15.00 mm" 9 0.05: shape e76add623f02c929 advance 405cb1ba5e353f7c metrics 405cb1ba5e353f7c 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "Row 12: Width 15.00 mm" 14 0: shape 59a25986e0740845 advance 4064648b43958106 metrics 4064648b43958106 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "Row 12: Width 15.00 mm" 14 0.05: shape 59a25986e0740845 advance 4066515810624dd3 metrics 4066515810624dd3 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "Row 12: Width 15.00 mm" 17.5 0: shape 11ee959abf1289b1 advance 40697dae147ae148 metrics 40697dae147ae148 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "Row 12: Width 15.00 mm" 17.5 0.05: shape 11ee959abf1289b1 advance 406be5ae147ae149 metrics 406be5ae147ae149 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "Row 12: Width 15.00 mm" 28 0: shape 3dc22c07613adb05 advance 4074648b43958106 metrics 4074648b43958106 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "Row 12: Width 15.00 mm" 28 0.05: shape 3dc22c07613adb05 advance 4076515810624dd3 metrics 4076515810624dd3 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "Row 12: Width 15.00 mm" 13.333333333333334 0: shape 2301b60972c7bdc5 advance 40636bf258bf258c metrics 40636bf258bf258c 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "Row 12: Width 15.00 mm" 13.333333333333334 0.05: shape 2301b60972c7bdc5 advance 40654147ae147ae1 metrics 40654147ae147ae1 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "Row 12: Width 15.00 mm" label x1: size 4064648b43958106 4035000000000000 ink 0000000000000000 0000000000000000
noto-tnum "Row 12: Width 15.00 mm" label x1.5: size 406e96d0e5604189 403f800000000000 ink 0000000000000000 0000000000000000
noto-tnum "0123456789" 9 0: shape e40fc5c10948f886 advance 4049bd70a3d70a3c metrics 4049bd70a3d70a3c 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "0123456789" 9 0.05: shape e40fc5c10948f886 advance 404bfd70a3d70a3d metrics 404bfd70a3d70a3d 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "0123456789" 14 0: shape 8401f7f13df0a8b6 advance 4054051eb851eb85 metrics 4054051eb851eb85 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "0123456789" 14 0.05: shape 8401f7f13df0a8b6 advance 4055c51eb851eb85 metrics 4055c51eb851eb85 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "0123456789" 17.5 0: shape eb019417e2f80676 advance 4059066666666669 metrics 4059066666666669 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "0123456789" 17.5 0.05: shape eb019417e2f80676 advance 405b366666666669 metrics 405b366666666669 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "0123456789" 28 0: shape aa62598ddcfcc136 advance 4064051eb851eb85 metrics 4064051eb851eb85 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "0123456789" 28 0.05: shape aa62598ddcfcc136 advance 4065c51eb851eb85 metrics 4065c51eb851eb85 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "0123456789" 13.333333333333334 0: shape e4a405391bd4d066 advance 4053111111111111 metrics 4053111111111111 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "0123456789" 13.333333333333334 0.05: shape e4a405391bd4d066 advance 4054bbbbbbbbbbbd metrics 4054bbbbbbbbbbbd 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "0123456789" label x1: size 4054051eb851eb85 4035000000000000 ink 0000000000000000 0000000000000000
noto-tnum "0123456789" label x1.5: size 405e07ae147ae148 403f800000000000 ink 0000000000000000 0000000000000000
noto-tnum "AV Wa fi ffl Ty" 9 0: shape e0ef06227a981ebd advance 404c5e353f7ced92 metrics 404c5e353f7ced92 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "AV Wa fi ffl Ty" 9 0.05: shape e0ef06227a981ebd advance 404fbe353f7ced90 metrics 404fbe353f7ced90 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "AV Wa fi ffl Ty" 14 0: shape fe59b05a177a5237 advance 405610624dd2f1ab metrics 405610624dd2f1ab 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "AV Wa fi ffl Ty" 14 0.05: shape fe59b05a177a5237 advance 4058b0624dd2f1aa metrics 4058b0624dd2f1aa 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "AV Wa fi ffl Ty" 17.5 0: shape 8458495ce0893408 advance 405b947ae147ae14 metrics 405b947ae147ae14 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "AV Wa fi ffl Ty" 17.5 0.05: shape 8458495ce0893408 advance 405edc7ae147ae13 metrics 405edc7ae147ae13 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "AV Wa fi ffl Ty" 28 0: shape 80d28648a39edc87 advance 406610624dd2f1ab metrics 406610624dd2f1ab 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "AV Wa fi ffl Ty" 28 0.05: shape 80d28648a39edc87 advance 4068b0624dd2f1aa metrics 4068b0624dd2f1aa 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "AV Wa fi ffl Ty" 13.333333333333334 0: shape b608bf9f1807f57a advance 40550369d0369d03 metrics 40550369d0369d03 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "AV Wa fi ffl Ty" 13.333333333333334 0.05: shape b608bf9f1807f57a advance 40578369d0369d04 metrics 40578369d0369d04 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "AV Wa fi ffl Ty" label x1: size 405610624dd2f1ab 4035000000000000 ink 0000000000000000 0000000000000000
noto-tnum "AV Wa fi ffl Ty" label x1.5: size 40608c49ba5e3540 403f800000000000 ink 0000000000000000 0000000000000000
noto-tnum "  spaced  out  " 9 0: shape 0e5fd75cad225f98 advance 404d0f9db22d0e58 metrics 404d0f9db22d0e58 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "  spaced  out  " 9 0.05: shape 0e5fd75cad225f98 advance 405037ced916872a metrics 405037ced916872a 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "  spaced  out  " 14 0: shape ad18aae5327855cc advance 40569a5e353f7cee metrics 40569a5e353f7cee 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "  spaced  out  " 14 0.05: shape ad18aae5327855cc advance 40593a5e353f7cef metrics 40593a5e353f7cef 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "  spaced  out  " 17.5 0: shape 913caf269dad940d advance 405c40f5c28f5c29 metrics 405c40f5c28f5c29 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "  spaced  out  " 17.5 0.05: shape 913caf269dad940d advance 405f88f5c28f5c29 metrics 405f88f5c28f5c29 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "  spaced  out  " 28 0: shape 7aba78e9628d0adc advance 40669a5e353f7cee metrics 40669a5e353f7cee 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "  spaced  out  " 28 0.05: shape 7aba78e9628d0adc advance 40693a5e353f7cef metrics 40693a5e353f7cef 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "  spaced  out  " 13.333333333333334 0: shape 73d3ee649e29370e advance 405586d3a06d3a08 metrics 405586d3a06d3a08 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "  spaced  out  " 13.333333333333334 0.05: shape 73d3ee649e29370e advance 405806d3a06d3a09 metrics 405806d3a06d3a09 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "  spaced  out  " label x1: size 40569a5e353f7cee 4035000000000000 ink 0000000000000000 0000000000000000
noto-tnum "  spaced  out  " label x1.5: size 4060f3c6a7ef9db2 403f800000000000 ink 0000000000000000 0000000000000000
noto-tnum "\u{f109} laptop" 9 0: shape 7c0078d8ac93e234 advance 40416fffffffffff metrics 40416fffffffffff 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "\u{f109} laptop" 9 0.05: shape 7c0078d8ac93e234 advance 40433ccccccccccc metrics 40433ccccccccccc 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "\u{f109} laptop" 14 0: shape d23443460a538f74 advance 404b200000000000 metrics 404b200000000000 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "\u{f109} laptop" 14 0.05: shape d23443460a538f74 advance 404deccccccccccc metrics 404deccccccccccc 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "\u{f109} laptop" 17.5 0: shape cc3393bc0dc7cfb7 advance 4050f40000000001 metrics 4050f40000000001 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "\u{f109} laptop" 17.5 0.05: shape cc3393bc0dc7cfb7 advance 4052b40000000001 metrics 4052b40000000001 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "\u{f109} laptop" 28 0: shape 78a3d3b04d3fbe74 advance 405b200000000000 metrics 405b200000000000 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "\u{f109} laptop" 28 0.05: shape 78a3d3b04d3fbe74 advance 405deccccccccccc metrics 405deccccccccccc 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "\u{f109} laptop" 13.333333333333334 0: shape 49425fee04a8b392 advance 4049d55555555556 metrics 4049d55555555556 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "\u{f109} laptop" 13.333333333333334 0.05: shape 49425fee04a8b392 advance 404c800000000001 metrics 404c800000000001 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "\u{f109} laptop" label x1: size 404b200000000000 4035000000000000 ink 0000000000000000 0000000000000000
noto-tnum "\u{f109} laptop" label x1.5: size 4054580000000001 403f800000000000 ink 0000000000000000 0000000000000000
noto-tnum "日本" 9 0: shape 15eea19a2e409e5d advance 4025999999999999 metrics 4025999999999999 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "日本" 9 0.05: shape 15eea19a2e409e5d advance 4027666666666666 metrics 4027666666666666 40233df3b645a1cb 4005189374bc6a7f 402884189374bc6a
noto-tnum "日本" 14 0: shape c5e3a2e2e9020815 advance 4030cccccccccccd metrics 4030cccccccccccd 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "日本" 14 0.05: shape c5e3a2e2e9020815 advance 4032333333333333 metrics 4032333333333333 402dee978d4fdf3b 40106872b020c49c 4033116872b020c5
noto-tnum "日本" 17.5 0: shape 3fe23b354f064335 advance 4035000000000001 metrics 4035000000000001 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "日本" 17.5 0.05: shape 3fe23b354f064335 advance 4036c00000000001 metrics 4036c00000000001 4032b51eb851eb85 4014828f5c28f5c3 4037d5c28f5c28f6
noto-tnum "日本" 28 0: shape 2b04ed1e9f4f0995 advance 4040cccccccccccd metrics 4040cccccccccccd 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "日本" 28 0.05: shape 2b04ed1e9f4f0995 advance 4042333333333333 metrics 4042333333333333 403dee978d4fdf3b 40206872b020c49c 4043116872b020c5
noto-tnum "日本" 13.333333333333334 0: shape 9743f2b4a368be25 advance 4030000000000000 metrics 4030000000000000 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "日本" 13.333333333333334 0.05: shape 9743f2b4a368be25 advance 4031555555555555 metrics 4031555555555555 402c81b4e81b4e82 400f40da740da742 403228f5c28f5c29
noto-tnum "日本" label x1: size 4030cccccccccccd 4035000000000000 ink 0000000000000000 0000000000000000
noto-tnum "日本" label x1.5: size 4039333333333334 403f800000000000 ink 0000000000000000 0000000000000000"#;

/// Swapping the system font remeasures a laid-out `Label`, even when the old
/// font is freed first and the new one is allocated at its address: the
/// label compares font ids, which are never reused, not pointers. Safe code
/// cannot force the allocator to reuse an address, so the label swaps
/// through many fonts of alternating faces, each freed before the next is
/// made, which is where a reused address would serve a stale width.
#[test]
fn system_font_swap_remeasures_label() {
    let own = Arc::new(Font::from_slice(NOTO).expect("noto"));
    let mut label = Label::new("Width 15.00 mm", Arc::clone(&own));
    let available = Size::new(1000.0, 100.0);
    let mut widths = Vec::new();
    for i in 0..32 {
        let bytes = if i % 2 == 0 { CASCADIA } else { NOTO };
        let font = Arc::new(Font::from_slice(bytes).expect("font"));
        font_settings::set_system_font(Some(Arc::clone(&font)));
        let width = label.layout(available).width;
        font_settings::set_system_font(None);
        assert_eq!(width, fresh_label_width(&font, available), "swap {i}");
        widths.push(width);
        // `font` is freed here, before the next one is allocated.
    }
    assert_ne!(
        widths[0], widths[1],
        "the two faces must measure differently"
    );
}

/// The width a new `Label` of the same text lays out to in `font`, with no
/// system font override.
fn fresh_label_width(font: &Arc<Font>, available: Size) -> f64 {
    Label::new("Width 15.00 mm", Arc::clone(font))
        .layout(available)
        .width
}

/// Swapping the system font invalidates a laid-out `TextField`'s cached
/// bitmap every time, even when the old font is freed first and the new one
/// is allocated at its address: the field's cache signature records the
/// font's id, not its pointer. Same many-swap scheme as
/// [`system_font_swap_remeasures_label`].
#[test]
fn system_font_swap_invalidates_text_field_cache() {
    let own = Arc::new(Font::from_slice(NOTO).expect("noto"));
    let mut field = TextField::new(own).with_text("Width 15.00 mm");
    field.set_bounds(Rect::new(0.0, 0.0, 200.0, 24.0));
    let available = Size::new(200.0, 24.0);
    for i in 0..32 {
        let bytes = if i % 2 == 0 { CASCADIA } else { NOTO };
        let font = Arc::new(Font::from_slice(bytes).expect("font"));
        font_settings::set_system_font(Some(Arc::clone(&font)));
        field.layout(available);
        font_settings::set_system_font(None);
        let cache = field.backbuffer_cache_mut().expect("text field caches");
        assert!(
            cache.dirty,
            "swap {i}: the new font must re-raster the field"
        );
        // What a paint does: the bitmap is current for this font now.
        cache.dirty = false;
        // `font` is freed here, before the next one is allocated.
    }
}

/// Swapping a `TreeView`'s font rebuilds its row widgets, so every row
/// measures in the new font, even when the old font is freed first: the
/// row signature hashes the font's id, not its pointer.
#[test]
fn tree_view_font_swap_remeasures_rows() {
    let available = Size::new(400.0, 200.0);
    let mut tree = tree_with_row(Arc::new(Font::from_slice(NOTO).expect("noto")));
    tree.layout(available);
    let mut widths = Vec::new();
    for i in 0..32 {
        let bytes = if i % 2 == 0 { CASCADIA } else { NOTO };
        let font = Arc::new(Font::from_slice(bytes).expect("font"));
        tree.font = Arc::clone(&font);
        tree.layout(available);
        let width = row_label_width(&tree);
        let mut fresh = tree_with_row(Arc::clone(&font));
        fresh.layout(available);
        assert_eq!(width, row_label_width(&fresh), "swap {i}");
        widths.push(width);
        // Let go of `font` before the next one is allocated; the tree's
        // rows release theirs when the next layout replaces them.
        drop(fresh);
        drop(font);
    }
    assert_ne!(
        widths[0], widths[1],
        "the two faces must measure differently"
    );
}

/// A tree with one root row, "Width 15.00 mm".
fn tree_with_row(font: Arc<Font>) -> TreeView {
    let mut tree = TreeView::new(font);
    tree.add_root("Width 15.00 mm", NodeIcon::File);
    tree
}

/// The laid-out width of the first row's name label (the row's last
/// child: indent, expand toggle, icon, label).
fn row_label_width(tree: &TreeView) -> f64 {
    let row = &tree.children()[0];
    row.children().last().expect("row label").bounds().width
}
