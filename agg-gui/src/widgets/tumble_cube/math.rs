//! Tiny 3-D math kit for the tumble cube — no external math crate.
//!
//! agg-gui deliberately carries no `glam`/`nalgebra` dependency, and the
//! cube only needs a handful of operations, so they live here as plain
//! `[f64; 3]` vectors and `[[f64; 3]; 3]` rotation matrices.
//!
//! # Matrix convention (load-bearing — the camera trait uses it)
//!
//! A [`Mat3`] is the upper-left 3x3 of agg-sharp's `Matrix4X4`, stored
//! exactly the same way: **row-major, row vectors**.  `m[r][c]` is
//! agg-sharp's `Matrix4X4[r, c]` (`RowN.Xyz`), and a point transforms as
//! `v' = v * M` ([`transform`]).  Composition therefore reads left to right:
//! `a * b` means "apply `a`, then `b`" — the same order agg-sharp code is
//! written in (`LookAt(...) * CreateScale(.8)`).
//!
//! Quaternions ([`Quat`]) are `[x, y, z, w]` (glam's memory order), with
//! [`quat_from_mat3`] / [`mat3_from_quat`] converting against [`Mat3`].

/// A 3-component vector.
pub type Vec3 = [f64; 3];
/// Row-major rotation matrix in agg-sharp's row-vector convention.
pub type Mat3 = [[f64; 3]; 3];
/// Unit quaternion `[x, y, z, w]`.
pub type Quat = [f64; 4];

/// The identity rotation.
pub const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: Vec3, s: f64) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn length(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}

/// Normalize `a`; a zero vector stays zero rather than becoming NaN.
pub fn normalize(a: Vec3) -> Vec3 {
    let l = length(a);
    if l > 0.0 {
        scale(a, 1.0 / l)
    } else {
        a
    }
}

/// `v * M` — agg-sharp's `Vector3.TransformNormal` / `TransformVector`
/// for the rotation part (row vector on the left).
pub fn transform(v: Vec3, m: &Mat3) -> Vec3 {
    [
        v[0] * m[0][0] + v[1] * m[1][0] + v[2] * m[2][0],
        v[0] * m[0][1] + v[1] * m[1][1] + v[2] * m[2][1],
        v[0] * m[0][2] + v[1] * m[1][2] + v[2] * m[2][2],
    ]
}

/// `a * b` in row-vector order: apply `a`, then `b`.
pub fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    out
}

pub fn transpose(m: &Mat3) -> Mat3 {
    [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ]
}

/// Rotation part of agg-sharp's `Matrix4X4.LookAt(Vector3.Zero, target, up)`.
///
/// Verbatim port: `z = normalize(eye - target)`, `x = normalize(up × z)`,
/// `y = normalize(z × x)`, and the matrix holds `x`, `y`, `z` as its
/// **columns** — so `v * M` takes a world vector into view space (the
/// camera looks down view `-Z`).  The translation part is zero because
/// the cube always uses `eye = 0`.
pub fn look_at(target: Vec3, up: Vec3) -> Mat3 {
    let z = normalize(scale(target, -1.0));
    let x = normalize(cross(up, z));
    let y = normalize(cross(z, x));
    [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]]
}

/// Unit quaternion for a row-vector rotation matrix; the inverse of
/// [`mat3_from_quat`].  Only the demo camera and apps use this — the
/// tumble cube itself talks to its camera in matrices (see [`Mat3`]).
pub fn quat_from_mat3(m: &Mat3) -> Quat {
    // Shepperd's method on the row-vector matrix; for row vectors the
    // matrix is the transpose of the column-vector one, which flips the
    // sign of the off-diagonal differences relative to the textbook form.
    let trace = m[0][0] + m[1][1] + m[2][2];
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (m[1][2] - m[2][1]) / s,
            (m[2][0] - m[0][2]) / s,
            (m[0][1] - m[1][0]) / s,
            0.25 * s,
        ]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [
            0.25 * s,
            (m[1][0] + m[0][1]) / s,
            (m[2][0] + m[0][2]) / s,
            (m[1][2] - m[2][1]) / s,
        ]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [
            (m[1][0] + m[0][1]) / s,
            0.25 * s,
            (m[2][1] + m[1][2]) / s,
            (m[2][0] - m[0][2]) / s,
        ]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [
            (m[2][0] + m[0][2]) / s,
            (m[2][1] + m[1][2]) / s,
            0.25 * s,
            (m[0][1] - m[1][0]) / s,
        ]
    };
    normalize_quat(q)
}

/// Row-vector rotation matrix for a unit quaternion (inverse of
/// [`quat_from_mat3`]).
pub fn mat3_from_quat(q: Quat) -> Mat3 {
    let [x, y, z, w] = normalize_quat(q);
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + w * z),
            2.0 * (x * z - w * y),
        ],
        [
            2.0 * (x * y - w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + w * x),
        ],
        [
            2.0 * (x * z + w * y),
            2.0 * (y * z - w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

fn normalize_quat(q: Quat) -> Quat {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l > 0.0 {
        [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// Shortest-path spherical interpolation, `t` in `0..=1` — the operation
/// MatterCAD's `AnimateRotation` steps with `Quaternion.Slerp`.
pub fn slerp(a: Quat, b: Quat, t: f64) -> Quat {
    let mut d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let mut b = b;
    if d < 0.0 {
        d = -d;
        b = [-b[0], -b[1], -b[2], -b[3]];
    }
    if d > 0.9995 {
        // Nearly parallel: lerp is exact enough and avoids 0/0.
        let l = [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
            a[3] + (b[3] - a[3]) * t,
        ];
        return normalize_quat(l);
    }
    let theta = d.acos();
    let s = theta.sin();
    let wa = ((1.0 - t) * theta).sin() / s;
    let wb = (t * theta).sin() / s;
    normalize_quat([
        a[0] * wa + b[0] * wb,
        a[1] * wa + b[1] * wb,
        a[2] * wa + b[2] * wb,
        a[3] * wa + b[3] * wb,
    ])
}
