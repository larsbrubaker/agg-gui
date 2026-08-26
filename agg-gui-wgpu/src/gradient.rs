//! Gradient pipeline helpers for the wgpu backend.
//!
//! Mirrors `demo-gl/src/gradient.rs`:  builds the 1-D ramp texture from SVG
//! gradient stops and packs the affine/gradient uniforms into `GradientUniforms`
//! for the `GradientPipeline` WGSL shader.

use bytemuck::{Pod, Zeroable};

use agg_gui::color::Color;
use agg_gui::draw_ctx::{GradientSpread, GradientStop, LinearGradientPaint, RadialGradientPaint};
use agg_gui::TransAffine;

/// Packed uniform struct for the gradient shader (group 0, binding 0).
///
/// Must be 16-byte aligned throughout.  Members are ordered to avoid implicit
/// padding; explicit `_pad` fields fill any gaps.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct GradientUniforms {
    pub resolution: [f32; 2],
    pub _pad0: [f32; 2],
    pub line: [f32; 4],   // [x1, y1, x2, y2]
    pub radial: [f32; 4], // [cx, cy, r, 0]
    pub focal: [f32; 2],  // [fx, fy]
    pub _pad1: [f32; 2],
    pub screen_inv_a: [f32; 4], // affine [sx, shy, shx, sy]
    pub screen_inv_b: [f32; 2], // affine [tx, ty]
    pub _pad2: [f32; 2],
    pub gradient_inv_a: [f32; 4],
    pub gradient_inv_b: [f32; 2],
    pub kind: u32,   // 0 = linear, 1 = radial
    pub spread: u32, // 0 = pad, 1 = reflect, 2 = repeat
    pub global_alpha: f32,
    /// Explicit padding to 144 bytes (vec3 alignment in WGSL would skip to 144).
    pub _pad3: [f32; 3],
}

const _: () = assert!(std::mem::size_of::<GradientUniforms>() == 144);

pub(crate) const RAMP_W: usize = 256;

/// Build a 256×1 RGBA8 gradient ramp from SVG-style stops.
pub(crate) fn gradient_ramp(stops: &[GradientStop]) -> Vec<u8> {
    let mut ramp = vec![0u8; RAMP_W * 4];
    for x in 0..RAMP_W {
        let t = x as f64 / (RAMP_W - 1) as f64;
        let color = sample_stops(stops, t);
        let i = x * 4;
        ramp[i] = (color.r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        ramp[i + 1] = (color.g.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        ramp[i + 2] = (color.b.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        ramp[i + 3] = (color.a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
    ramp
}

fn sample_stops(stops: &[GradientStop], t: f64) -> Color {
    if t <= stops[0].offset {
        return stops[0].color;
    }
    for pair in stops.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if t <= b.offset {
            let span = (b.offset - a.offset).max(f64::EPSILON);
            let u = ((t - a.offset) / span).clamp(0.0, 1.0) as f32;
            return Color::rgba(
                a.color.r + (b.color.r - a.color.r) * u,
                a.color.g + (b.color.g - a.color.g) * u,
                a.color.b + (b.color.b - a.color.b) * u,
                a.color.a + (b.color.a - a.color.a) * u,
            );
        }
    }
    stops[stops.len() - 1].color
}

/// Pack a `TransAffine` into the two-vector form used in `GradientUniforms`.
///
/// Returns `(a, b)` where `a = [sx, shy, shx, sy]` and `b = [tx, ty]`.
/// Matches the packing in `demo-gl/src/gradient.rs::set_affine_uniforms`.
fn pack_affine(m: &TransAffine) -> ([f32; 4], [f32; 2]) {
    (
        [m.sx as f32, m.shy as f32, m.shx as f32, m.sy as f32],
        [m.tx as f32, m.ty as f32],
    )
}

fn spread_to_u32(s: GradientSpread) -> u32 {
    match s {
        GradientSpread::Pad => 0,
        GradientSpread::Reflect => 1,
        GradientSpread::Repeat => 2,
    }
}

/// Build `GradientUniforms` for a linear gradient.
pub(crate) fn build_linear_gradient_uniforms(
    gradient: &LinearGradientPaint,
    screen_from_local: &TransAffine,
    resolution: [f32; 2],
    global_alpha: f32,
) -> GradientUniforms {
    let mut screen_to_local = *screen_from_local;
    screen_to_local.invert();
    let mut gradient_inverse = gradient.transform;
    gradient_inverse.invert();

    let (si_a, si_b) = pack_affine(&screen_to_local);
    let (gi_a, gi_b) = pack_affine(&gradient_inverse);

    GradientUniforms {
        resolution,
        _pad0: [0.0; 2],
        line: [
            gradient.x1 as f32,
            gradient.y1 as f32,
            gradient.x2 as f32,
            gradient.y2 as f32,
        ],
        radial: [0.0; 4],
        focal: [0.0; 2],
        _pad1: [0.0; 2],
        screen_inv_a: si_a,
        screen_inv_b: si_b,
        _pad2: [0.0; 2],
        gradient_inv_a: gi_a,
        gradient_inv_b: gi_b,
        kind: 0,
        spread: spread_to_u32(gradient.spread),
        global_alpha,
        _pad3: [0.0; 3],
    }
}

/// Build `GradientUniforms` for a radial gradient.
pub(crate) fn build_radial_gradient_uniforms(
    gradient: &RadialGradientPaint,
    screen_from_local: &TransAffine,
    resolution: [f32; 2],
    global_alpha: f32,
) -> GradientUniforms {
    let mut screen_to_local = *screen_from_local;
    screen_to_local.invert();
    let mut gradient_inverse = gradient.transform;
    gradient_inverse.invert();

    let (si_a, si_b) = pack_affine(&screen_to_local);
    let (gi_a, gi_b) = pack_affine(&gradient_inverse);

    GradientUniforms {
        resolution,
        _pad0: [0.0; 2],
        line: [0.0; 4],
        radial: [
            gradient.cx as f32,
            gradient.cy as f32,
            gradient.r as f32,
            0.0,
        ],
        focal: [gradient.fx as f32, gradient.fy as f32],
        _pad1: [0.0; 2],
        screen_inv_a: si_a,
        screen_inv_b: si_b,
        _pad2: [0.0; 2],
        gradient_inv_a: gi_a,
        gradient_inv_b: gi_b,
        kind: 1,
        spread: spread_to_u32(gradient.spread),
        global_alpha,
        _pad3: [0.0; 3],
    }
}
