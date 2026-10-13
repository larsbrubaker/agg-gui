//! `fill_text` under a rotated or sheared transform draws rotated on the wgpu
//! backend, as it does on the software `GfxCtx`.
//!
//! Headless GPU readback of "HHHH" drawn through `DrawCtx::fill_text` under a
//! 90 degree and a 30 degree rotation (and a shear) at 1x, 1.5x and 2x, compared
//! with the software backend's rendering of the same calls.  The comparison is
//! the inked bounding box and ink centroid, not exact pixels: the two backends
//! rasterise with different anti-aliasing.  A text that ignores the rotation
//! inks a wide, short box; a rotated one inks the software backend's box.
//!
//! Axis-aligned text (pure translate and scale) keeps the coverage-mask path:
//! its readback is pinned, by checksum, against a capture taken before the
//! rotation routing existed.  `AGG_GUI_CAPTURE_DIR` makes the tests write the
//! raw RGBA readbacks (and print the checksums) for inspection.
//!
//! The readback tests pass trivially when no GPU adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::text::Font;
use agg_gui::{Framebuffer, GfxCtx};

use crate::layer_text_readback_tests::{try_device, Target};
use crate::WgpuGfxCtx;

const TEST_FONT: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
const LOGICAL: u32 = 96;

/// How the "HHHH" is placed: the transform the call runs under.
#[derive(Clone, Copy, Debug)]
enum Place {
    Rotate(f64),
    /// A horizontal shear (`shx`) on top of the scale, no rotation.
    Shear(f64),
    Plain,
}

fn apply(ctx: &mut dyn DrawCtx, place: Place, scale: f64) {
    ctx.scale(scale, scale);
    ctx.translate(48.0, 48.0);
    match place {
        Place::Rotate(deg) => ctx.rotate(deg.to_radians()),
        Place::Shear(k) => {
            let mut t = ctx.transform();
            t.shx = k;
            // Same translate and scale as above, plus the shear.
            ctx.set_transform(t);
        }
        Place::Plain => {}
    }
}

/// Gray `255 - luma` image, top row first, from either backend.
struct Ink {
    w: u32,
    h: u32,
    dark: Vec<u8>,
}

impl Ink {
    fn from_rgba_top_first(data: &[u8], w: u32, h: u32) -> Self {
        let dark = data.chunks_exact(4).map(|p| 255 - p[1]).collect();
        Self { w, h, dark }
    }

    /// (min_x, max_x, min_y, max_y, centroid_x, centroid_y) of the ink.
    fn stats(&self) -> Option<[f64; 6]> {
        let (mut x0, mut x1, mut y0, mut y1) = (u32::MAX, 0, u32::MAX, 0);
        let (mut sum, mut cx, mut cy) = (0.0, 0.0, 0.0);
        for y in 0..self.h {
            for x in 0..self.w {
                let d = self.dark[(y * self.w + x) as usize];
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
}

fn software(place: Place, scale: f64, font: &Arc<Font>) -> Ink {
    let side = (LOGICAL as f64 * scale) as u32;
    let mut fb = Framebuffer::new(side, side);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.set_lcd_mode(false);
        ctx.clear(Color::white());
        ctx.set_font(Arc::clone(font));
        ctx.set_font_size(12.0);
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 1.0));
        apply(&mut ctx, place, scale);
        DrawCtx::fill_text(&mut ctx, "HHHH", -20.0, -4.0);
    }
    Ink::from_rgba_top_first(&fb.pixels_flipped(), side, side)
}

fn wgpu_ctx(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    side: u32,
    font: &Arc<Font>,
) -> WgpuGfxCtx {
    let mut ctx = WgpuGfxCtx::new(
        Arc::clone(device),
        Arc::clone(queue),
        wgpu::TextureFormat::Rgba8Unorm,
        side as f32,
        side as f32,
    );
    ctx.reset(side as f32, side as f32);
    ctx.set_lcd_mode(false);
    ctx.clear(Color::white());
    ctx.set_font(Arc::clone(font));
    ctx.set_font_size(12.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 1.0));
    ctx
}

fn wgpu_render(place: Place, scale: f64, font: &Arc<Font>) -> Option<(Ink, Vec<u8>)> {
    let (device, queue) = try_device()?;
    let side = (LOGICAL as f64 * scale) as u32;
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), side, side);
    let mut ctx = wgpu_ctx(&device, &queue, side, font);
    apply(&mut ctx, place, scale);
    ctx.fill_text("HHHH", -20.0, -4.0);
    ctx.flush_to_surface(&target.view);
    let data = target.read();
    if let Ok(dir) = std::env::var("AGG_GUI_CAPTURE_DIR") {
        let name = format!("{place:?}_{scale}x").replace(['(', ')', '.'], "_");
        std::fs::write(format!("{dir}/wgpu_{name}.rgba"), &data).unwrap();
    }
    Some((Ink::from_rgba_top_first(&data, side, side), data))
}

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

fn assert_same_ink(place: Place, scale: f64) -> bool {
    let font = font();
    let Some((gpu, _)) = wgpu_render(place, scale, &font) else {
        eprintln!("SKIP: no GPU adapter");
        return false;
    };
    let sw = software(place, scale, &font);
    let g = gpu.stats().expect("wgpu drew no ink");
    let s = sw.stats().expect("software drew no ink");
    // The two rasterisers' anti-aliasing puts an edge pixel in or out: the box
    // agrees to one pixel and the centroid to half of one.
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
            (g[i] - s[i]).abs() <= if i < 4 { 1.0 } else { 0.75 },
            "{place:?} at {scale}x: ink {name} is {} on wgpu and {} on the software \
             backend (wgpu {g:?}, software {s:?})",
            g[i],
            s[i]
        );
    }
    true
}

#[test]
fn rotated_90_degrees_text_matches_the_software_backend_at_1x_1_5x_and_2x() {
    for &scale in &[1.0, 1.5, 2.0] {
        if !assert_same_ink(Place::Rotate(90.0), scale) {
            return;
        }
    }
}

#[test]
fn rotated_30_degrees_text_matches_the_software_backend_at_1x_1_5x_and_2x() {
    for &scale in &[1.0, 1.5, 2.0] {
        if !assert_same_ink(Place::Rotate(30.0), scale) {
            return;
        }
    }
}

#[test]
fn sheared_text_matches_the_software_backend_at_1x_1_5x_and_2x() {
    for &scale in &[1.0, 1.5, 2.0] {
        if !assert_same_ink(Place::Shear(0.5), scale) {
            return;
        }
    }
}

#[test]
fn rotated_90_degrees_text_is_taller_than_wide() {
    // The unrotated "HHHH" is wide and short; turned a quarter it is the reverse.
    let font = font();
    let Some((ink, _)) = wgpu_render(Place::Rotate(90.0), 1.0, &font) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let s = ink.stats().expect("no ink");
    assert!(
        s[3] - s[2] > 2.0 * (s[1] - s[0]),
        "rotated text ink box is {}x{}, not tall and narrow",
        s[1] - s[0],
        s[3] - s[2]
    );
}

/// FNV-1a over the readback, for the pre-change capture comparison.
fn checksum(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

#[test]
fn axis_aligned_text_keeps_the_mask_path_pixel_for_pixel() {
    // (scale, FNV-1a of the Rgba8 readback captured before rotated text was
    // routed to the outline path).
    const BEFORE: [(f64, u64); 3] = [
        (1.0, 0x75373130315a8e25),
        (1.5, 0x106bfd8e4eac9bbb),
        (2.0, 0xf584995c68191daa),
    ];
    let font = font();
    let mut got = Vec::new();
    for &(scale, _) in &BEFORE {
        let Some((_, data)) = wgpu_render(Place::Plain, scale, &font) else {
            eprintln!("SKIP: no GPU adapter");
            return;
        };
        if std::env::var("AGG_GUI_CAPTURE_DIR").is_ok() {
            eprintln!("capture {scale}x checksum {:#018x}", checksum(&data));
        }
        got.push(checksum(&data));
    }
    for (&(scale, expected), got) in BEFORE.iter().zip(got) {
        assert_eq!(
            got, expected,
            "axis-aligned text at {scale}x no longer matches the capture taken before the change"
        );
    }
}

#[test]
fn only_a_rotated_or_sheared_transform_leaves_the_mask_path() {
    use crate::text_render::ctm_needs_outline_text;
    use agg_gui::TransAffine;
    let mut scaled = TransAffine::new();
    scaled.sx = 1.5;
    scaled.sy = -1.5;
    scaled.tx = 10.25;
    assert!(!ctm_needs_outline_text(&scaled), "translate and scale");
    for deg in [30.0f64, 90.0, 180.0, 270.0] {
        let (s, c) = deg.to_radians().sin_cos();
        let mut t = TransAffine::new();
        (t.sx, t.shy, t.shx, t.sy) = (c, s, -s, c);
        // A half turn is a pure (negative) scale and stays on the mask path.
        assert_eq!(ctm_needs_outline_text(&t), deg != 180.0, "{deg} degrees");
    }
    let mut full_turn = TransAffine::new();
    full_turn.shy = (2.0 * std::f64::consts::PI).sin();
    assert!(
        !ctm_needs_outline_text(&full_turn),
        "float noise of a full turn"
    );
    let mut sheared = TransAffine::new();
    sheared.shx = 0.5;
    assert!(ctm_needs_outline_text(&sheared), "shear");
}
