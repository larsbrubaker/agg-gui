//! Tiny 4×4 matrix + 3-vector helpers used by [`crate::bar_grid`].
//!
//! Pulled out of `bar_grid.rs` to keep that file under the project's 800-line
//! limit.  These helpers don't pull in `nalgebra` or `glam` so the wgpu demo's
//! transitive deps stay minimal.

pub(crate) type Mat4 = [f32; 16];

pub(crate) fn mat4_mul(a: Mat4, b: Mat4) -> Mat4 {
    let mut out = [0f32; 16];
    for row in 0..4 {
        for col in 0..4 {
            out[col * 4 + row] = a[row] * b[col * 4]
                + a[4 + row] * b[col * 4 + 1]
                + a[8 + row] * b[col * 4 + 2]
                + a[12 + row] * b[col * 4 + 3];
        }
    }
    out
}

/// Right-handed perspective for WebGPU/wgpu NDC where Z is in `[0, 1]`.  The
/// `far * nf` formulation (vs. GL's `(far + near) * nf`) is what makes
/// `depth_compare = Less` work out of the box without flipping near/far.
pub(crate) fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y * 0.5).tan();
    let nf = 1.0 / (near - far);
    [
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        far * nf,
        -1.0,
        0.0,
        0.0,
        far * near * nf,
        0.0,
    ]
}

pub(crate) fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
    let f = normalize3(sub3(target, eye));
    let s = normalize3(cross3(f, up));
    let u = cross3(s, f);
    [
        s[0],
        u[0],
        -f[0],
        0.0,
        s[1],
        u[1],
        -f[1],
        0.0,
        s[2],
        u[2],
        -f[2],
        0.0,
        -dot3(s, eye),
        -dot3(u, eye),
        dot3(f, eye),
        1.0,
    ]
}

#[inline]
fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
pub(crate) fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / len, v[1] / len, v[2] / len]
}
