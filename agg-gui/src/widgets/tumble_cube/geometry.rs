//! Cube geometry and lighting shared by the CPU and GPU renderers.
//!
//! The C# cube is `PlatonicSolids.CreateCube(4, 4, 4)` with one label
//! texture fitted to each face.  Which way each label faces is not free:
//! `DrawMouseHover` paints tile 0 at the image's bottom-left, and
//! `GetHitData` calls the tile at the corner a face shares with its
//! `left` and `bottom` neighbours tile 0 — so each face's texture
//! bottom-left must sit at that corner.  The table below is the unique
//! layout that satisfies [`super::hit_test::CONNECTIONS`] for every face
//! (it is also AtomArtist's `cube_geometry.rs` table, rescaled to ±2); a
//! test checks it against the connection table.
//!
//! Lighting is agg-sharp's default `LightingData` as the WebGPU scene
//! renderer's `WriteLightUniform` publishes it, with `applyLighting` from
//! `NodeDesignerScene.wgsl` ported for the CPU path.

use super::hit_test::CUBE_HALF_SIZE;
use super::math::{self, Vec3};

/// One face: texture bottom-left corner, the unit directions the texture's
/// +U (right) and +V (up) run along, and the outward normal.
#[derive(Clone, Copy, Debug)]
pub struct FaceFrame {
    pub origin: Vec3,
    pub u: Vec3,
    pub v: Vec3,
    pub normal: Vec3,
}

const H: f64 = CUBE_HALF_SIZE;

/// Face frames in face-index order (Top, Left, Right, Bottom, Back, Front).
pub const FACE_FRAMES: [FaceFrame; 6] = [
    FaceFrame {
        origin: [-H, -H, H],
        u: [1.0, 0.0, 0.0],
        v: [0.0, 1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
    },
    FaceFrame {
        origin: [-H, H, -H],
        u: [0.0, -1.0, 0.0],
        v: [0.0, 0.0, 1.0],
        normal: [-1.0, 0.0, 0.0],
    },
    FaceFrame {
        origin: [H, -H, -H],
        u: [0.0, 1.0, 0.0],
        v: [0.0, 0.0, 1.0],
        normal: [1.0, 0.0, 0.0],
    },
    FaceFrame {
        origin: [-H, H, -H],
        u: [1.0, 0.0, 0.0],
        v: [0.0, -1.0, 0.0],
        normal: [0.0, 0.0, -1.0],
    },
    FaceFrame {
        origin: [H, H, -H],
        u: [-1.0, 0.0, 0.0],
        v: [0.0, 0.0, 1.0],
        normal: [0.0, 1.0, 0.0],
    },
    FaceFrame {
        origin: [-H, -H, -H],
        u: [1.0, 0.0, 0.0],
        v: [0.0, 0.0, 1.0],
        normal: [0.0, -1.0, 0.0],
    },
];

impl FaceFrame {
    /// Texture coordinates (`0..1`, V up) of a point on this face.
    pub fn uv(&self, p: Vec3) -> (f64, f64) {
        let d = math::sub(p, self.origin);
        let edge = 2.0 * H;
        (math::dot(d, self.u) / edge, math::dot(d, self.v) / edge)
    }

    /// The four corners `[BL, BR, TR, TL]` in texture terms.
    pub fn corners(&self) -> [Vec3; 4] {
        let e = 2.0 * H;
        let bl = self.origin;
        let br = math::add(bl, math::scale(self.u, e));
        let tl = math::add(bl, math::scale(self.v, e));
        let tr = math::add(br, math::scale(self.v, e));
        [bl, br, tr, tl]
    }
}

/// agg-sharp `LightingData` defaults, already in eye space (the C# sets
/// the lights under an identity modelview).
#[derive(Clone, Copy, Debug)]
pub struct CubeLighting {
    pub light0_direction: Vec3,
    pub sky_ambient: f64,
    pub light0_diffuse: f64,
    pub light1_direction: Vec3,
    pub ground_ambient: f64,
    pub light1_diffuse: f64,
}

impl Default for CubeLighting {
    fn default() -> Self {
        Self {
            light0_direction: [-1.0, -1.0, 1.0],
            sky_ambient: 0.2,
            light0_diffuse: 0.7,
            light1_direction: [1.0, 1.0, 1.0],
            ground_ambient: 0.2,
            light1_diffuse: 0.5,
        }
    }
}

impl CubeLighting {
    /// `applyLighting` from `NodeDesignerScene.wgsl` with both lights on
    /// and no specular / rim (the defaults): the multiplier for a surface
    /// whose eye-space normal is `n`.
    pub fn factor(&self, n: Vec3) -> f64 {
        let n = math::normalize(n);
        let t = n[1] * 0.5 + 0.5;
        let hemisphere = self.ground_ambient + (self.sky_ambient - self.ground_ambient) * t;
        let d0 = math::dot(n, math::normalize(self.light0_direction)).max(0.0);
        let d1 = math::dot(n, math::normalize(self.light1_direction)).max(0.0);
        hemisphere + self.light0_diffuse * d0 + self.light1_diffuse * d1
    }
}
