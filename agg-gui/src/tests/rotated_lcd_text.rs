//! `fill_text` under a rotated or sheared transform draws rotated through both
//! software LCD paths (`GfxCtx` with `lcd_mode` on, and `LcdGfxCtx`), as it
//! does through `GfxCtx` with `lcd_mode` off, and as the wgpu backend does.
//!
//! "HHHH" is drawn under a 90 degree and a 30 degree rotation and a shear at
//! 1x, 1.5x and 2x.  A path that blits its unrotated mask inks a wide, short
//! box; a rotated one inks the `lcd_mode`-off box.  The comparison is the inked
//! bounding box (within 1 px) and ink centroid (within 0.75 px): the LCD paths
//! fall back to grayscale outline text but rasterise through a different
//! anti-aliasing pipeline, so exact pixels differ.
//!
//! Axis-aligned text (pure translate and scale) keeps the LCD mask path: its
//! pixels are pinned by FNV-1a checksums captured before the rotation routing
//! existed.  `AGG_GUI_PRINT_CHECKSUMS=1` prints the current checksums.

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::lcd_coverage::LcdBuffer;
use crate::lcd_gfx_ctx::LcdGfxCtx;
use crate::text::Font;
use std::sync::Arc;

const TEST_FONT: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");
const LOGICAL: u32 = 96;

/// How the "HHHH" is placed: the transform the call runs under.
#[derive(Clone, Copy, Debug)]
enum Place {
    Rotate(f64),
    /// A horizontal shear (`shx`) on top of the scale, no rotation.
    Shear(f64),
    Plain,
}

#[derive(Clone, Copy, Debug)]
enum Path {
    /// `GfxCtx` with `lcd_mode` off: the reference.
    GfxGray,
    /// `GfxCtx` with `lcd_mode` on.
    GfxLcd,
    /// `LcdGfxCtx` over an `LcdBuffer`.
    LcdBuffer,
}

fn draw(ctx: &mut dyn DrawCtx, place: Place, scale: f64, font: &Arc<Font>) {
    ctx.clear(Color::white());
    ctx.set_font(Arc::clone(font));
    ctx.set_font_size(12.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 1.0));
    ctx.scale(scale, scale);
    ctx.translate(48.0, 48.0);
    match place {
        Place::Rotate(deg) => ctx.rotate(deg.to_radians()),
        Place::Shear(k) => {
            let mut t = ctx.transform();
            t.shx = k;
            ctx.set_transform(t);
        }
        Place::Plain => {}
    }
    ctx.fill_text("HHHH", -20.0, -4.0);
}

/// RGBA8, top row first, of "HHHH" drawn through `path`.
fn render(path: Path, place: Place, scale: f64, font: &Arc<Font>) -> (Vec<u8>, u32) {
    let side = (LOGICAL as f64 * scale) as u32;
    let data = match path {
        Path::GfxGray | Path::GfxLcd => {
            let mut fb = Framebuffer::new(side, side);
            {
                let mut ctx = GfxCtx::new(&mut fb);
                ctx.set_lcd_mode(matches!(path, Path::GfxLcd));
                draw(&mut ctx, place, scale, font);
            }
            fb.pixels_flipped()
        }
        Path::LcdBuffer => {
            let mut buf = LcdBuffer::new(side, side);
            {
                let mut ctx = LcdGfxCtx::new(&mut buf);
                draw(&mut ctx, place, scale, font);
            }
            buf.to_rgba8_top_down_collapsed()
        }
    };
    (data, side)
}

/// (min_x, max_x, min_y, max_y, centroid_x, centroid_y) of the dark pixels.
fn stats(data: &[u8], side: u32) -> Option<[f64; 6]> {
    let (mut x0, mut x1, mut y0, mut y1) = (u32::MAX, 0, u32::MAX, 0);
    let (mut sum, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for y in 0..side {
        for x in 0..side {
            let d = 255 - data[((y * side + x) * 4 + 1) as usize];
            if d > 32 {
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
            sum += d as f64;
            cx += d as f64 * x as f64;
            cy += d as f64 * y as f64;
        }
    }
    if x1 < x0 || sum == 0.0 {
        return None;
    }
    Some([
        x0 as f64,
        x1 as f64,
        y0 as f64,
        y1 as f64,
        cx / sum,
        cy / sum,
    ])
}

fn assert_same_ink(path: Path, place: Place, scale: f64) {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let (got, side) = render(path, place, scale, &font);
    let (want, _) = render(Path::GfxGray, place, scale, &font);
    let g = stats(&got, side).expect("the LCD path drew no ink");
    let w = stats(&want, side).expect("the lcd_mode-off path drew no ink");
    for (i, name) in [
        "min x",
        "max x",
        "min y",
        "max y",
        "centroid x",
        "centroid y",
    ]
    .iter()
    .enumerate()
    {
        assert!(
            (g[i] - w[i]).abs() <= if i < 4 { 1.0 } else { 0.75 },
            "{path:?} {place:?} at {scale}x: ink {name} is {} and {} with lcd_mode off \
             ({g:?} against {w:?})",
            g[i],
            w[i]
        );
    }
}

fn assert_same_ink_at_all_scales(place: Place) {
    for path in [Path::GfxLcd, Path::LcdBuffer] {
        for &scale in &[1.0, 1.5, 2.0] {
            assert_same_ink(path, place, scale);
        }
    }
}

#[test]
fn rotated_90_degrees_lcd_text_matches_lcd_mode_off_at_1x_1_5x_and_2x() {
    assert_same_ink_at_all_scales(Place::Rotate(90.0));
}

#[test]
fn rotated_30_degrees_lcd_text_matches_lcd_mode_off_at_1x_1_5x_and_2x() {
    assert_same_ink_at_all_scales(Place::Rotate(30.0));
}

#[test]
fn sheared_lcd_text_matches_lcd_mode_off_at_1x_1_5x_and_2x() {
    assert_same_ink_at_all_scales(Place::Shear(0.5));
}

#[test]
fn rotated_lcd_text_has_no_chroma() {
    // Outline text is grayscale: every pixel's channels agree, where the LCD
    // mask path would fringe the glyph edges.
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    for path in [Path::GfxLcd, Path::LcdBuffer] {
        let (data, _) = render(path, Place::Rotate(30.0), 1.0, &font);
        let spread = data
            .chunks_exact(4)
            .map(|p| p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2]))
            .max()
            .unwrap();
        assert!(spread <= 2, "{path:?}: rotated text has chroma {spread}");
    }
}

/// FNV-1a over the readback, for the pre-change capture comparison.
fn checksum(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

#[test]
fn axis_aligned_lcd_text_keeps_the_mask_path_pixel_for_pixel() {
    // (path, scale, FNV-1a of the readback captured before rotated text was
    // routed to the outline path).
    const BEFORE: [(Path, f64, u64); 6] = [
        (Path::GfxLcd, 1.0, 0xad7f31674b372dea),
        (Path::GfxLcd, 1.5, 0x122668ef39e009e7),
        (Path::GfxLcd, 2.0, 0x149e5dc919fc6e0b),
        (Path::LcdBuffer, 1.0, 0xad7f31674b372dea),
        (Path::LcdBuffer, 1.5, 0x122668ef39e009e7),
        (Path::LcdBuffer, 2.0, 0x149e5dc919fc6e0b),
    ];
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let got: Vec<u64> = BEFORE
        .iter()
        .map(|&(path, scale, _)| {
            let (data, side) = render(path, Place::Plain, scale, &font);
            assert!(
                stats(&data, side).is_some(),
                "{path:?} at {scale}x drew no ink"
            );
            checksum(&data)
        })
        .collect();
    if std::env::var("AGG_GUI_PRINT_CHECKSUMS").is_ok() {
        for (&(path, scale, _), got) in BEFORE.iter().zip(&got) {
            eprintln!("capture {path:?} {scale}x checksum {got:#018x}");
        }
    }
    for (&(path, scale, expected), got) in BEFORE.iter().zip(got) {
        assert_eq!(
            got, expected,
            "axis-aligned {path:?} text at {scale}x no longer matches the capture taken \
             before the change"
        );
    }
}
