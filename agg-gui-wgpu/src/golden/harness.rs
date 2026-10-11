//! Shared harness for the ported agg-sharp GoldenImages classes
//! (`Tests/Agg.Tests/Agg.GoldenImages`): renders one scene through the wgpu
//! backend (`WgpuGfxCtx`) and through the software `GfxCtx`, reads both back
//! top-row-first, and compares them.
//!
//! C# compares its WebGPU render with PNGs captured from its own renderer
//! (`Graphics2DGpu`). agg-gui's GPU path tessellates and anti-aliases its own
//! way, so those PNGs can't be matched byte for byte; the reference here is
//! agg-gui's software rasterizer, which is the spec the GPU path must follow.
//! Siblings: `golden/*_tests.rs` hold the ported test classes.
//!
//! Skipped (the render returns `None`) when no GPU adapter is available.
//! Set `GOLDEN_DUMP_DIR` to a directory to have `assert_matches` write both
//! renders there as PPM images, for looking at a failure.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::framebuffer::Framebuffer;
use agg_gui::gfx_ctx::GfxCtx;

use crate::layer_text_readback_tests::{try_device, Target};
use crate::WgpuGfxCtx;

/// The largest channel difference allowed on an edge pixel: C#'s own
/// GPU-against-software tolerance for halo anti-aliasing
/// (`GpuFillRuleTests.Tolerance`, with its reasoning in that class's remarks).
pub(crate) const EDGE_TOLERANCE: i32 = 128;

/// One scene rendered both ways, both top-row-first RGBA8.
pub(crate) struct Pair {
    pub w: u32,
    pub h: u32,
    pub software: Vec<u8>,
    pub gpu: Vec<u8>,
}

/// Software render of `draw` on a `w`×`h` surface cleared to `bg`, flipped
/// to top-row-first like the GPU readback.
pub(crate) fn software(w: u32, h: u32, bg: Color, draw: &dyn Fn(&mut dyn DrawCtx)) -> Vec<u8> {
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(bg);
        draw(&mut ctx);
    }
    fb.pixels_flipped()
}

/// GPU render of the same scene, or `None` without an adapter.
pub(crate) fn gpu(w: u32, h: u32, bg: Color, draw: &dyn Fn(&mut dyn DrawCtx)) -> Option<Vec<u8>> {
    let (device, queue) = try_device()?;
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), w, h);
    let mut ctx = WgpuGfxCtx::new(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        w as f32,
        h as f32,
    );
    ctx.reset(w as f32, h as f32);
    ctx.clear(bg);
    draw(&mut ctx);
    ctx.flush_to_surface(&target.view);
    Some(target.read())
}

/// Both renders, or `None` (with a SKIP line) without an adapter.
pub(crate) fn render_both(
    w: u32,
    h: u32,
    bg: Color,
    draw: &dyn Fn(&mut dyn DrawCtx),
) -> Option<Pair> {
    let Some(gpu) = gpu(w, h, bg, draw) else {
        eprintln!("SKIP: no GPU adapter");
        return None;
    };
    Some(Pair {
        w,
        h,
        software: software(w, h, bg, draw),
        gpu,
    })
}

/// Pixel `(x, y)` (top-row-first) of a readback.
pub(crate) fn at(data: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [data[i], data[i + 1], data[i + 2], data[i + 3]]
}

/// Largest per-channel difference between two pixels (RGB only).
pub(crate) fn diff(a: [u8; 4], b: [u8; 4]) -> i32 {
    (0..3)
        .map(|c| (a[c] as i32 - b[c] as i32).abs())
        .max()
        .unwrap_or(0)
}

impl Pair {
    /// Requires every interior pixel within `tolerance` and every edge pixel
    /// within [`EDGE_TOLERANCE`]. A pixel is an edge pixel when the software
    /// render's 5×5 neighbourhood is not flat: the GPU anti-aliases with a
    /// halo (agg-gui `gl_renderer::halo_aa_tesselator`), not AGG's area
    /// coverage, so near an edge the two legitimately disagree by up to half
    /// the shade's range.
    pub(crate) fn assert_matches(&self, what: &str, tolerance: i32) {
        if let Ok(dir) = std::env::var("GOLDEN_DUMP_DIR") {
            for (tag, data) in [("sw", &self.software), ("gpu", &self.gpu)] {
                let mut out = format!("P6 {} {} 255\n", self.w, self.h).into_bytes();
                for p in data.chunks(4) {
                    out.extend_from_slice(&p[..3]);
                }
                std::fs::write(format!("{dir}/{what}.{tag}.ppm"), out).ok();
            }
        }
        let mut edge_diffs = 0usize;
        let mut max = 0;
        for y in 0..self.h {
            for x in 0..self.w {
                let g = at(&self.gpu, self.w, x, y);
                let s = at(&self.software, self.w, x, y);
                let d = diff(g, s);
                max = max.max(d);
                if d <= tolerance {
                    continue;
                }
                let flat = self
                    .neighbours(x, y)
                    .iter()
                    .all(|n| diff(*n, s) <= tolerance);
                assert!(
                    !flat,
                    "{what}: interior pixel {x},{y} (top-row-first) GPU {g:?} against software {s:?}"
                );
                assert!(
                    d <= EDGE_TOLERANCE,
                    "{what}: edge pixel {x},{y} (top-row-first) GPU {g:?} against software {s:?}"
                );
                edge_diffs += 1;
            }
        }
        eprintln!("{what}: {edge_diffs} edge pixels differ, max channel diff {max}");
    }

    fn neighbours(&self, x: u32, y: u32) -> Vec<[u8; 4]> {
        let mut out = Vec::with_capacity(25);
        for ny in y.saturating_sub(2)..=(y + 2).min(self.h - 1) {
            for nx in x.saturating_sub(2)..=(x + 2).min(self.w - 1) {
                out.push(at(&self.software, self.w, nx, ny));
            }
        }
        out
    }
}
