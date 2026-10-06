//! The cube's own little camera: how the main view's rotation becomes the
//! cube's modelview / projection, and how a widget pixel becomes a ray.
//!
//! Port of the `WorldView` the C# `TumbleCubeControl.OnDraw` builds every
//! frame: `new WorldView(width, height)` (agg-sharp defaults — 45° vertical
//! FOV, near 0.1, far 100, camera pulled back by `CameraZTranslationFudge
//! = -7`) with `RotationMatrix = LookAt(0, forward, up) * Scale(.8)`, where
//! `forward`/`up` are view `-Z`/`+Y` taken back into world space through
//! the main camera.  Rebuilding the rotation from forward/up (instead of
//! copying it) is what the C# does, and it strips any zoom scale the
//! main modelview carries.

use super::math::{self, Mat3, Vec3};

/// `WorldView.DefaultPerspectiveVFOVDegrees`.
pub const VFOV_DEGREES: f64 = 45.0;
/// `WorldView.DefaultNearZ` / `DefaultFarZ`.
pub const NEAR_Z: f64 = 0.1;
pub const FAR_Z: f64 = 100.0;
/// `WorldView.CameraZTranslationFudge`: the eye sits 7 units back.
pub const CAMERA_DISTANCE: f64 = 7.0;
/// The `Matrix4X4.CreateScale(.8)` the C# appends to the rotation.
pub const CUBE_SCALE: f64 = 0.8;

/// A snapshot of the cube camera for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubeView {
    /// World → view rotation of the cube (orthonormal, no scale).
    pub rotation: Mat3,
    pub width: f64,
    pub height: f64,
}

impl CubeView {
    /// Build the cube view for a main-camera rotation (see
    /// [`super::TumbleCubeCamera::view_rotation`] for the convention).
    pub fn new(main_view_rotation: &Mat3, width: f64, height: f64) -> Self {
        // TransformNormal(v, InverseModelview): for a rotation (possibly
        // uniformly scaled) the inverse's direction part is the transpose.
        let inv = math::transpose(main_view_rotation);
        let forward = math::transform([0.0, 0.0, -1.0], &inv);
        let up = math::transform([0.0, 1.0, 0.0], &inv);
        Self {
            rotation: math::look_at(forward, up),
            width,
            height,
        }
    }

    fn tan_half_fov() -> f64 {
        (VFOV_DEGREES.to_radians() * 0.5).tan()
    }

    /// Eye position in cube (mesh) coordinates.
    pub fn eye(&self) -> Vec3 {
        // view = 0.8 * (p R) + (0, 0, -7)  =>  p = ((view - t) / 0.8) Rᵀ
        let rt = math::transpose(&self.rotation);
        math::transform([0.0, 0.0, CAMERA_DISTANCE / CUBE_SCALE], &rt)
    }

    /// The ray through widget-local `(x, y)` (Y-up, origin bottom-left) in
    /// cube mesh coordinates — `WorldView.GetRayForLocalBounds`.
    pub fn ray(&self, x: f64, y: f64) -> (Vec3, Vec3) {
        let w = self.width.max(1.0);
        let h = self.height.max(1.0);
        let ndc_x = 2.0 * x / w - 1.0;
        let ndc_y = 2.0 * y / h - 1.0;
        let t = Self::tan_half_fov();
        let dir_view = [ndc_x * t * (w / h), ndc_y * t, -1.0];
        let dir = math::normalize(math::transform(dir_view, &math::transpose(&self.rotation)));
        (self.eye(), dir)
    }

    /// The face/tile under widget-local `(x, y)`, or `None` on a miss.
    pub fn hit(&self, x: f64, y: f64) -> Option<super::hit_test::HitData> {
        let (o, d) = self.ray(x, y);
        super::hit_test::intersect_cube(o, d).map(super::hit_test::get_hit_data)
    }

    /// The 4x4 modelview in agg-sharp's row-major, row-vector layout
    /// (`v_view = v_mesh * M`), as the GPU uniform wants it.
    pub fn modelview(&self) -> [[f64; 4]; 4] {
        let r = &self.rotation;
        let s = CUBE_SCALE;
        [
            [r[0][0] * s, r[0][1] * s, r[0][2] * s, 0.0],
            [r[1][0] * s, r[1][1] * s, r[1][2] * s, 0.0],
            [r[2][0] * s, r[2][1] * s, r[2][2] * s, 0.0],
            [0.0, 0.0, -CAMERA_DISTANCE, 1.0],
        ]
    }

    /// Right-handed perspective projection with clip depth mapped to
    /// `0..w` (what agg-sharp's `GlUniformBlock.ToClipSpaceProjection`
    /// hands the WGSL), in the same row-vector layout as [`Self::modelview`].
    pub fn projection(&self) -> [[f64; 4]; 4] {
        let f = 1.0 / Self::tan_half_fov();
        let aspect = self.width.max(1.0) / self.height.max(1.0);
        let (n, fa) = (NEAR_Z, FAR_Z);
        [
            [f / aspect, 0.0, 0.0, 0.0],
            [0.0, f, 0.0, 0.0],
            [0.0, 0.0, fa / (n - fa), -1.0],
            [0.0, 0.0, n * fa / (n - fa), 0.0],
        ]
    }
}
