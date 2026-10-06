//! The camera interface a host app implements to drive (and be driven by)
//! a [`super::TumbleCube`].
//!
//! The cube owns no camera.  MatterCAD's C# control talks to its
//! `WorldView` (to read the rotation) and `TrackballTumbleWidgetExtended`
//! (`StartRotateAroundOrigin` / `DoRotateAroundOrigin` /
//! `EndRotateAroundOrigin` for drags, `AnimateRotation` for clicks); this
//! trait is exactly that surface, so a port can forward each call 1:1.

use crate::geometry::Point;

use super::math::Mat3;

/// What the tumble cube needs from the app's 3-D camera.
///
/// # Rotation convention
///
/// Rotations are [`Mat3`]: the **upper-left 3x3 of agg-sharp's
/// `Matrix4X4`, row-major with row vectors** — `m[r][c] == Matrix4X4[r, c]`
/// and `v_view = v_world * m`.  In MatterCAD terms:
///
/// * [`view_rotation`](Self::view_rotation) returns the rotation part of
///   the main `WorldView`'s **world → view** transform (its
///   `RotationMatrix`, or equivalently the modelview's 3x3).  A uniform
///   scale in it is harmless — the cube rebuilds an orthonormal rotation
///   from its forward/up axes, as the C# does.
/// * [`animate_rotation`](Self::animate_rotation) receives the value the
///   C# passes to `AnimateRotation(Matrix4X4)`: `Matrix4X4.LookAt(0,
///   normal, up)`, a world → view rotation with no translation — so it
///   can be copied straight into a `Matrix4X4` and handed to the same
///   slerp the C# runs.
///
/// World space is Z-up (Top = +Z, Front = -Y), as in MatterCAD.
pub trait TumbleCubeCamera {
    /// The main view's current world → view rotation.
    fn view_rotation(&self) -> Mat3;

    /// A drag began on the cube — C# `StartRotateAroundOrigin(position)`.
    /// `pos` is in the cube widget's local coordinates (Y-up, origin at
    /// its bottom-left), the same frame the C# receives.
    fn begin_rotate(&mut self, pos: Point);

    /// The drag moved — C# `DoRotateAroundOrigin(position)`.
    fn rotate(&mut self, pos: Point);

    /// The drag ended — C# `EndRotateAroundOrigin()`.
    fn end_rotate(&mut self);

    /// Turn the view to `target` (a world → view rotation), animated —
    /// C# `AnimateRotation(Matrix4X4)`.  A new request supersedes one
    /// still running, as `RunCameraMove` does.
    fn animate_rotation(&mut self, target: Mat3);
}
