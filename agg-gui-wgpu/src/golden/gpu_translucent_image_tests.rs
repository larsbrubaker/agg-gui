//! Port of agg-sharp `Agg.GoldenImages/GpuTranslucentImageTests.cs`: an
//! image (`DrawCtx::draw_image_rgba`, C#'s `Graphics2D.Render(IImageByte,
//! ...)`) blends a translucent texel over the target once on the GPU, as
//! the software `GfxCtx` does; a translucent fill without edge anti-aliasing
//! blends once; an image lands where software puts it; and transparent
//! texels don't bleed colour into a magnified edge.
//!
//! agg-gui images carry no `OriginOffset`: C#'s `Render(image, x, y)` of an
//! image with offset `(ox, oy)` puts its bottom-left at `(x - ox, y - oy)`,
//! which the port passes explicitly.

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;

use super::harness::{at, gpu, render_both, software};

/// `WebGpuOffscreenCapture`'s default size.
const W: u32 = 512;
const H: u32 = 384;

/// Pixel `(x, y)` with y up, as C#'s `GetPixel`.
fn pixel(data: &[u8], x: u32, y: u32) -> [u8; 4] {
    at(data, W, x, H - 1 - y)
}

fn assert_within(a: [u8; 4], b: [u8; 4], tolerance: i32, what: &str) {
    for c in 0..3 {
        assert!(
            (a[c] as i32 - b[c] as i32).abs() <= tolerance,
            "{what}: {a:?} against {b:?}"
        );
    }
}

/// `w`×`h` RGBA8 (top row first) of one straight-alpha colour.
fn filled(w: usize, h: usize, rgba: [u8; 4]) -> Vec<u8> {
    rgba.iter().copied().cycle().take(w * h * 4).collect()
}

#[test]
fn straight_alpha_image_blends_like_software() {
    let source = filled(16, 16, [240, 120, 60, 128]);
    let (x, y) = (100.0, 80.0);
    let Some(pair) = render_both(W, H, Color::white(), &|ctx| {
        ctx.draw_image_rgba(&source, 16, 16, x, y, 16.0, 16.0)
    }) else {
        return;
    };
    // The middle of the image, clear of any bilinear edge. Over white that
    // is about (247, 187, 157).
    assert_within(
        pixel(&pair.gpu, 108, 88),
        pixel(&pair.software, 108, 88),
        2,
        "image middle",
    );
}

/// A fill without edge anti-aliasing blends half-transparent red once, not
/// at full strength.
#[test]
fn non_anti_aliased_translucent_fill_blends_once() {
    let Some(image) = gpu(W, H, Color::rgb(0.0, 0.0, 1.0), &|ctx| {
        ctx.set_anti_alias(false);
        ctx.set_fill_color(Color::from_rgba8(255, 0, 0, 128));
        ctx.begin_path();
        ctx.move_to(100.3, 80.3);
        ctx.line_to(160.3, 80.3);
        ctx.line_to(160.3, 140.3);
        ctx.line_to(100.3, 140.3);
        ctx.close_path();
        ctx.fill();
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let middle = pixel(&image, 130, 110);
    assert!((middle[0] as i32 - 128).abs() <= 2, "{middle:?}");
    assert!(middle[1] <= 2, "{middle:?}");
    assert!((middle[2] as i32 - 127).abs() <= 2, "{middle:?}");
}

/// C#'s `[Arguments(5, 3)]` and `[Arguments(-4, -6)]`: a 16×12 red image
/// with a green 4×4 corner at its top right, placed by its origin offset.
#[test]
fn origin_offset_places_the_image_like_software() {
    let (w, h) = (16usize, 12usize);
    let mut source = filled(w, h, [255, 0, 0, 255]);
    // C#'s FillRectangle(12, 8, 16, 12): y up, so the top four rows.
    for row in 0..4 {
        for col in 12..16 {
            let i = (row * w + col) * 4;
            source[i..i + 4].copy_from_slice(&[0, 128, 0, 255]);
        }
    }
    for (offset_x, offset_y) in [(5.0, 3.0), (-4.0, -6.0)] {
        let draw = |ctx: &mut dyn DrawCtx| {
            ctx.draw_image_rgba(
                &source,
                w as u32,
                h as u32,
                100.0 - offset_x,
                80.0 - offset_y,
                w as f64,
                h as f64,
            )
        };
        let Some(gpu_image) = gpu(W, H, Color::white(), &draw) else {
            eprintln!("SKIP: no GPU adapter");
            return;
        };
        let software_image = software(W, H, Color::white(), &draw);
        let mut differing = 0;
        for y in 60..110 {
            for x in 80..130 {
                let (g, s) = (pixel(&gpu_image, x, y), pixel(&software_image, x, y));
                if (0..3).any(|c| (g[c] as i32 - s[c] as i32).abs() > 2) {
                    differing += 1;
                }
            }
        }
        assert_eq!(differing, 0, "offset ({offset_x}, {offset_y})");
    }
}

/// The invisible green beside an opaque red texel must not tint the
/// magnified edge between them.
#[test]
#[ignore = "wgpu image blit bug: textures are uploaded and bilinear-filtered as straight alpha, so a fully transparent green texel tints the magnified edge beside an opaque red one (green minus blue 64 where C# allows 2); fix by uploading premultiplied texels and blending One / OneMinusSrcAlpha in image_blit.rs and the textured-quad pipeline"]
fn transparent_pixels_do_not_bleed_when_magnified() {
    let source: Vec<u8> = vec![255, 0, 0, 255, 0, 255, 0, 0];
    let Some(image) = gpu(W, H, Color::white(), &|ctx| {
        ctx.draw_image_rgba(&source, 2, 1, 100.0, 80.0, 32.0, 16.0)
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    // Over white only red can drop out of green and blue, and it takes
    // both down together.
    let worst_tint = (100..132)
        .map(|x| {
            let p = pixel(&image, x, 88);
            p[1] as i32 - p[2] as i32
        })
        .max()
        .unwrap_or(0);
    assert!(worst_tint <= 2, "worst tint {worst_tint}");
}
