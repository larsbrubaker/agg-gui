//! Software renderer for the tumble cube — the fallback when no GPU hook
//! is installed (software `GfxCtx`, headless tests).
//!
//! It ray-casts the cube per pixel with the same camera ([`CubeView`]) and
//! hit test the widget uses for picking, so what is drawn is exactly what
//! is clickable.  Like the C# control (which captures at 3x and box
//! downsamples) it supersamples 3x3, samples the face textures bilinearly
//! and lights each face with the scene shader's model
//! ([`CubeLighting::factor`]).  Output is straight-alpha RGBA8, top row
//! first, transparent where the cube is not.

use super::face_textures::{FaceTexture, FACE_SIZE};
use super::geometry::{CubeLighting, FACE_FRAMES};
use super::hit_test::{intersect_cube, CONNECTIONS};
use super::math;
use super::view::CubeView;

/// Linear supersample factor (the C# full-frame capture is 3x).
pub const SSAA: u32 = 3;

/// Which face a surface point is on: the axis the intersection snapped.
fn face_of(p: [f64; 3]) -> usize {
    CONNECTIONS
        .iter()
        .position(|c| (p[c.axis] - c.direction * super::hit_test::CUBE_HALF_SIZE).abs() < 1e-9)
        .unwrap_or(0)
}

/// Bilinear sample of a `FACE_SIZE²` top-row-first image at `(u, v)`, V up.
/// Returns premultiplied RGBA in `0..1`.
fn sample(image: &[u8], u: f64, v: f64) -> [f64; 4] {
    let s = FACE_SIZE as f64;
    let x = (u * s - 0.5).clamp(0.0, s - 1.0);
    let y = ((1.0 - v) * s - 0.5).clamp(0.0, s - 1.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(FACE_SIZE - 1), (y0 + 1).min(FACE_SIZE - 1));
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let px = |xx: u32, yy: u32| {
        let i = ((yy * FACE_SIZE + xx) * 4) as usize;
        let a = image[i + 3] as f64 / 255.0;
        [
            image[i] as f64 / 255.0 * a,
            image[i + 1] as f64 / 255.0 * a,
            image[i + 2] as f64 / 255.0 * a,
            a,
        ]
    };
    let (p00, p10, p01, p11) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
    let mut out = [0.0; 4];
    for c in 0..4 {
        let top = p00[c] + (p10[c] - p00[c]) * fx;
        let bot = p01[c] + (p11[c] - p01[c]) * fx;
        out[c] = top + (bot - top) * fy;
    }
    out
}

/// Render the cube into a `width x height` RGBA8 image (pixels, not
/// logical units — `view` is scaled to match by the caller).
pub fn render(view: &CubeView, faces: &[FaceTexture], width: u32, height: u32) -> Vec<u8> {
    let lighting = CubeLighting::default();
    // Eye-space normals are constant per face: light each face once.
    let light: Vec<f64> = FACE_FRAMES
        .iter()
        .map(|f| lighting.factor(math::transform(f.normal, &view.rotation)))
        .collect();
    let sx = view.width / width.max(1) as f64;
    let sy = view.height / height.max(1) as f64;
    let mut out = vec![0u8; (width * height * 4) as usize];
    let n = (SSAA * SSAA) as f64;
    for row in 0..height {
        for col in 0..width {
            let mut acc = [0.0f64; 4];
            for j in 0..SSAA {
                for i in 0..SSAA {
                    // Sub-sample centres; Y counts up from the bottom.
                    let x = (col as f64 + (i as f64 + 0.5) / SSAA as f64) * sx;
                    let y = ((height - 1 - row) as f64 + (j as f64 + 0.5) / SSAA as f64) * sy;
                    let (o, d) = view.ray(x, y);
                    let Some(p) = intersect_cube(o, d) else {
                        continue;
                    };
                    let face = face_of(p);
                    let Some(tex) = faces.get(face) else { continue };
                    let (u, v) = FACE_FRAMES[face].uv(p);
                    let texel = sample(&tex.active, u, v);
                    let l = light[face];
                    // The shader lights straight colour and keeps alpha;
                    // in premultiplied terms that is the same scale.
                    acc[0] += (texel[0] * l).min(texel[3]);
                    acc[1] += (texel[1] * l).min(texel[3]);
                    acc[2] += (texel[2] * l).min(texel[3]);
                    acc[3] += texel[3];
                }
            }
            let a = acc[3] / n;
            let i = ((row * width + col) * 4) as usize;
            if a > 0.0 {
                for c in 0..3 {
                    out[i + c] = ((acc[c] / n / a).clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                out[i + 3] = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
    out
}
