//! Rasterizer clip-box tests for the software draw paths.
//!
//! `gfx_ctx/raster_clip.rs` clips edges in AGG's cell rasterizer so paths
//! that run far off screen stop costing time proportional to their length.
//! These tests compare against an unclipped rasterizer (renderer-level
//! scissor only, the previous behaviour): paths inside the clip box render
//! bit-for-bit the same, paths crossing it differ by at most one unit per
//! channel along the crossing edges.  They also pin that the cell bounds
//! stay inside the clip box, and carry an `#[ignore]`d
//! microbenchmark (`cargo test -p agg-gui --release rust_bench_ -- --ignored
//! --nocapture`).

use super::*;
use crate::draw_ctx::DrawCtx;
use crate::gfx_ctx::{apply_clip, clip_rasterizer, raster_clip_rect};
use crate::TransAffine;
use agg_rust::basics::FillingRule;
use agg_rust::comp_op::{CompOp, PixfmtRgba32CompOp};
use agg_rust::conv_curve::ConvCurve;
use agg_rust::conv_dash::ConvDash;
use agg_rust::conv_stroke::ConvStroke;
use agg_rust::conv_transform::ConvTransform;
use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::path_storage::PathStorage;
use agg_rust::rasterizer_scanline_aa::RasterizerScanlineAa;
use agg_rust::renderer_base::RendererBase;
use agg_rust::renderer_scanline::render_scanlines_aa_solid;
use agg_rust::rendering_buffer::RowAccessor;
use agg_rust::scanline_u::ScanlineU8;

const W: u32 = 1300;
const H: u32 = 800;

/// One draw in a test scene, rendered both through `GfxCtx` and through a
/// bare, unclipped AGG pipeline.
enum Op {
    Fill(Vec<(f64, f64)>),
    Stroke(Vec<(f64, f64)>, f64, Vec<f64>),
}

/// `GfxCtx`'s default stroke style (round joins and caps, miter limit 4).
fn style_stroke<VS: agg_rust::basics::VertexSource>(s: &mut ConvStroke<VS>, width: f64) {
    s.set_width(width);
    s.set_line_join(LineJoin::Round);
    s.set_line_cap(LineCap::Round);
    s.set_miter_limit(4.0);
}

fn path_of(points: &[(f64, f64)], close: bool) -> PathStorage {
    let mut p = PathStorage::new();
    p.move_to(points[0].0, points[0].1);
    for &(x, y) in &points[1..] {
        p.line_to(x, y);
    }
    if close {
        p.close_polygon(0);
    }
    p
}

/// Draw `ops` through the production `GfxCtx` under `transform` and `clip`.
fn render_gfx(
    ops: &[Op],
    transform: TransAffine,
    clip: Option<(f64, f64, f64, f64)>,
) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H);
    {
        let mut gfx = GfxCtx::new(&mut fb);
        let ctx: &mut dyn DrawCtx = &mut gfx;
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        if let Some((x, y, w, h)) = clip {
            ctx.clip_rect(x, y, w, h);
        }
        ctx.set_transform(transform);
        ctx.set_fill_color(Color::rgba(0.1, 0.3, 0.9, 0.7));
        ctx.set_stroke_color(Color::rgba(0.8, 0.1, 0.2, 0.9));
        for op in ops {
            ctx.begin_path();
            match op {
                Op::Fill(pts) => {
                    ctx.move_to(pts[0].0, pts[0].1);
                    for &(x, y) in &pts[1..] {
                        ctx.line_to(x, y);
                    }
                    ctx.close_path();
                    ctx.fill();
                }
                Op::Stroke(pts, width, dashes) => {
                    ctx.move_to(pts[0].0, pts[0].1);
                    for &(x, y) in &pts[1..] {
                        ctx.line_to(x, y);
                    }
                    ctx.set_line_width(*width);
                    ctx.set_line_dash(dashes, 0.0);
                    ctx.stroke();
                }
            }
        }
    }
    fb
}

/// The same scene through AGG with only the renderer scissor — no
/// rasterizer clip box — matching what `GfxCtx` did before clipping moved
/// into the rasterizer.
fn render_unclipped(
    ops: &[Op],
    transform: TransAffine,
    clip: Option<(f64, f64, f64, f64)>,
) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H);
    {
        let mut gfx = GfxCtx::new(&mut fb);
        gfx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
    }
    let fill = Color::rgba(0.1, 0.3, 0.9, 0.7).to_rgba8();
    let stroke_color = Color::rgba(0.8, 0.1, 0.2, 0.9).to_rgba8();
    for op in ops {
        let stride = (W * 4) as i32;
        let mut ra = RowAccessor::new();
        // SAFETY: `fb` owns W*H*4 bytes and outlives `ra`/`rb` in this block.
        unsafe { ra.attach(fb.pixels_mut().as_mut_ptr(), W, H, stride) };
        let pf = PixfmtRgba32CompOp::new_with_op(&mut ra, CompOp::SrcOver);
        let mut rb = RendererBase::new(pf);
        apply_clip(&mut rb, clip);
        let mut ras = RasterizerScanlineAa::new();
        ras.filling_rule(FillingRule::NonZero);
        let mut sl = ScanlineU8::new();
        match op {
            Op::Fill(pts) => {
                let mut path = path_of(pts, true);
                let mut curves = ConvCurve::new(&mut path);
                let mut tr = ConvTransform::new(&mut curves, transform);
                ras.add_path(&mut tr, 0);
                render_scanlines_aa_solid(&mut ras, &mut sl, &mut rb, &fill);
            }
            Op::Stroke(pts, width, dashes) => {
                let mut path = path_of(pts, false);
                let mut curves = ConvCurve::new(&mut path);
                if dashes.is_empty() {
                    let mut s = ConvStroke::new(&mut curves);
                    style_stroke(&mut s, *width);
                    let mut tr = ConvTransform::new(&mut s, transform);
                    ras.add_path(&mut tr, 0);
                } else {
                    let mut dash = ConvDash::new(&mut curves);
                    for pair in dashes.chunks_exact(2) {
                        dash.add_dash(pair[0], pair[1]);
                    }
                    dash.dash_start(0.0);
                    let mut s = ConvStroke::new(dash);
                    style_stroke(&mut s, *width);
                    let mut tr = ConvTransform::new(&mut s, transform);
                    ras.add_path(&mut tr, 0);
                }
                render_scanlines_aa_solid(&mut ras, &mut sl, &mut rb, &stroke_color);
            }
        }
    }
    fb
}

/// Lines and polygons that run far outside a 1300×800 target on every side
/// but still cross it, at fractional coordinates.
fn far_scene() -> Vec<Op> {
    vec![
        Op::Stroke(vec![(-200000.0, -20000.0), (1000.3, 600.7)], 1.5, vec![]),
        Op::Stroke(vec![(-200000.0, 400.25), (240000.0, 410.75)], 2.0, vec![]),
        Op::Stroke(vec![(650.5, -180000.0), (655.5, 180000.0)], 1.0, vec![]),
        Op::Stroke(
            vec![(100.0, 900000.0), (1200.0, -50000.0)],
            3.0,
            vec![12.0, 6.0],
        ),
        Op::Fill(vec![
            (-150000.0, -90000.0),
            (300.4, 200.6),
            (170000.0, -60000.0),
            (900.2, 120000.0),
        ]),
        Op::Fill(vec![
            (-5000.0, 100.0),
            (8000.0, 130.0),
            (8000.0, 700.0),
            (-5000.0, 690.0),
        ]),
    ]
}

/// Largest per-channel difference between two buffers, and how many channel
/// bytes differ at all.
fn channel_diff(a: &Framebuffer, b: &Framebuffer) -> (u8, usize) {
    let mut max = 0u8;
    let mut count = 0usize;
    for (x, y) in a.pixels().iter().zip(b.pixels()) {
        let d = x.abs_diff(*y);
        if d != 0 {
            count += 1;
            max = max.max(d);
        }
    }
    (max, count)
}

fn assert_drew_something(fb: &Framebuffer, what: &str) {
    // Guard against a vacuous pass where nothing was drawn at all.
    assert!(
        fb.pixels().iter().any(|&c| c != 255),
        "{what}: scene drew nothing"
    );
}

/// Edges that cross the rasterizer clip box restart at the boundary
/// intersection, rounded to AGG's 1/256-pixel grid, so the cell walk along
/// them can round one coverage step differently than the walk from the
/// original far-away endpoint.  That is inherent to AGG's clipper (C# and
/// C++ agg render the same way); the visible result may differ by at most
/// one unit in a channel, never more.
fn assert_within_one_lsb(a: &Framebuffer, b: &Framebuffer, what: &str) {
    let (max, count) = channel_diff(a, b);
    assert!(
        max <= 1,
        "{what}: {count} channel bytes differ, by up to {max}"
    );
    assert_drew_something(a, what);
}

fn assert_identical(a: &Framebuffer, b: &Framebuffer, what: &str) {
    let (max, count) = channel_diff(a, b);
    assert_eq!(
        count, 0,
        "{what}: {count} channel bytes differ, by up to {max}"
    );
    assert_drew_something(a, what);
}

#[test]
fn rust_only_on_screen_paths_identical_to_unclipped_raster() {
    // Paths that stay inside the clip box never touch the clipper, so they
    // must rasterize bit-for-bit as before — including the AA fringe of a
    // stroke that ends exactly at the framebuffer edge.
    let ops = vec![
        Op::Stroke(vec![(0.0, 0.5), (1299.5, 799.5)], 1.5, vec![]),
        Op::Stroke(
            vec![(10.25, 700.75), (1290.5, 40.25), (640.0, 400.0)],
            4.0,
            vec![9.0, 3.0],
        ),
        Op::Fill(vec![(0.0, 0.0), (1300.0, 0.0), (650.3, 799.9)]),
        Op::Fill(vec![
            (200.4, 200.6),
            (900.1, 250.2),
            (700.7, 600.3),
            (250.5, 550.5),
        ]),
    ];
    let t = TransAffine::new();
    assert_identical(
        &render_gfx(&ops, t, None),
        &render_unclipped(&ops, t, None),
        "on screen",
    );
}

#[test]
fn rust_only_far_offscreen_paths_match_unclipped_raster() {
    let ops = far_scene();
    let t = TransAffine::new();
    assert_within_one_lsb(
        &render_gfx(&ops, t, None),
        &render_unclipped(&ops, t, None),
        "no scissor",
    );
}

#[test]
fn rust_only_far_offscreen_paths_match_unclipped_raster_under_scissor() {
    let ops = far_scene();
    let t = TransAffine::new();
    let clip = Some((120.3, 75.6, 900.2, 500.9));
    assert_within_one_lsb(
        &render_gfx(&ops, t, clip),
        &render_unclipped(&ops, t, clip),
        "scissor",
    );
}

#[test]
fn rust_only_far_offscreen_paths_match_unclipped_raster_transformed() {
    // The clip box is in device space: a scaled + rotated + translated CTM
    // must clip after the transform, not before.
    let ops = far_scene();
    let mut t = TransAffine::new_scaling(1.7, 1.3);
    t.multiply(&TransAffine::new_rotation(0.3));
    t.multiply(&TransAffine::new_translation(80.0, -40.0));
    assert_within_one_lsb(
        &render_gfx(&ops, t, None),
        &render_unclipped(&ops, t, None),
        "transformed",
    );
    let clip = Some((200.0, 100.0, 700.0, 450.0));
    assert_within_one_lsb(
        &render_gfx(&ops, t, clip),
        &render_unclipped(&ops, t, clip),
        "transformed+scissor",
    );
}

#[test]
fn rust_only_raster_clip_rect_is_scissor_within_target_plus_margin() {
    assert_eq!(
        raster_clip_rect(None, 1300, 800),
        (-1.0, -1.0, 1301.0, 801.0)
    );
    assert_eq!(
        raster_clip_rect(Some((10.4, 20.6, 100.2, 50.0)), 1300, 800),
        (9.0, 19.0, 112.0, 72.0)
    );
    // Scissor partly off the target: intersected first.
    assert_eq!(
        raster_clip_rect(Some((-50.0, 700.0, 100.0, 500.0)), 1300, 800),
        (-1.0, 699.0, 51.0, 801.0)
    );
    // Disjoint scissor: degenerate box.
    let (x1, _, x2, _) = raster_clip_rect(Some((2000.0, 0.0, 10.0, 10.0)), 1300, 800);
    assert_eq!(x2 - x1, 2.0 * crate::gfx_ctx::RASTER_CLIP_MARGIN);
}

#[test]
fn rust_only_clipped_rasterizer_cells_stay_inside_clip_box() {
    let mut path = path_of(
        &[(-200000.0, -20000.0), (40000.0, -4000.0), (650.0, 400.0)],
        false,
    );
    let mut stroke = ConvStroke::new(&mut path);
    stroke.set_width(1.0);
    let mut ras = RasterizerScanlineAa::new();
    clip_rasterizer(&mut ras, None, W, H);
    ras.add_path(&mut stroke, 0);
    ras.rewind_scanlines();
    assert!(
        ras.min_x() >= -1 && ras.max_x() <= W as i32 + 1,
        "x {}..{}",
        ras.min_x(),
        ras.max_x()
    );
    assert!(
        ras.min_y() >= -1 && ras.max_y() <= H as i32 + 1,
        "y {}..{}",
        ras.min_y(),
        ras.max_y()
    );
}

/// Microbenchmark from the mattercad report: a stroke from
/// (−200000, −20000) to (40000, −4000) on a 1300×800 buffer.
#[test]
#[ignore]
fn rust_bench_far_offscreen_stroke() {
    let ops = vec![Op::Stroke(
        vec![(-200000.0, -20000.0), (40000.0, -4000.0)],
        1.0,
        vec![],
    )];
    let t = TransAffine::new();
    let iters = 20;
    let start = std::time::Instant::now();
    for _ in 0..iters {
        std::hint::black_box(render_unclipped(&ops, t, None));
    }
    let unclipped = start.elapsed() / iters;
    let start = std::time::Instant::now();
    for _ in 0..iters {
        std::hint::black_box(render_gfx(&ops, t, None));
    }
    let clipped = start.elapsed() / iters;
    let start = std::time::Instant::now();
    for _ in 0..iters {
        std::hint::black_box(render_gfx(&[], t, None));
    }
    let empty = start.elapsed() / iters;
    println!("per frame: unclipped {unclipped:?}, GfxCtx {clipped:?}, empty frame {empty:?}");
}
