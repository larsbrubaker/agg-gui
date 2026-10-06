//! Clicked face / edge / corner → the view rotation to turn to.
//!
//! Port of MatterCAD's `TumbleCubeControl.GetDirectionForFace` and the
//! `Matrix4X4.LookAt(Vector3.Zero, normal, up)` that `OnMouseUp` hands to
//! `AnimateRotation`.  (AtomArtist's `orient.rs` instead averaged eye
//! directions into its own orbit-camera quaternion and never used the C#
//! `up` vectors; the C# wins, so this follows the C# switch exactly.)

use super::hit_test::HitData;
use super::math::{self, Mat3, Vec3};

const X: Vec3 = [1.0, 0.0, 0.0];
const Y: Vec3 = [0.0, 1.0, 0.0];
const Z: Vec3 = [0.0, 0.0, 1.0];

/// The averaged view direction (`normal`, which the camera looks *along*)
/// and the up vector for a hit — C# `GetDirectionForFace`.
///
/// `normal` is the per-face forward direction summed over the hit faces
/// and divided by the face count (not normalized, as in the C#; `LookAt`
/// normalizes).  `up` comes from the first face only.
pub fn direction_for_face(hit: &HitData) -> (Vec3, Vec3) {
    let mut up = [0.0; 3];
    let mut normal = [0.0; 3];
    let mut count = 0;
    for i in 0..3 {
        count += 1;
        let first = count == 1;
        let (n, u) = match hit.face_index[i] {
            -1 => {
                count -= 1;
                continue;
            }
            // Top: look straight down.  A centre click keeps +Y up the
            // screen; an edge/corner click tilts toward it with +Z up.
            0 => (
                math::scale(Z, -1.0),
                if hit.tile_index[0] == 4 { Y } else { Z },
            ),
            1 => (X, Z),                    // Left: look toward +X
            2 => (math::scale(X, -1.0), Z), // Right
            3 => (Z, math::scale(Y, -1.0)), // Bottom: look up
            4 => (math::scale(Y, -1.0), Z), // Back
            5 => (Y, Z),                    // Front: look toward +Y
            _ => continue,
        };
        normal = math::add(normal, n);
        if first {
            up = u;
        }
    }
    if count == 0 {
        return (normal, up);
    }
    (math::scale(normal, 1.0 / count as f64), up)
}

/// The rotation `OnMouseUp` animates to for a click: `LookAt(0, normal,
/// up)`, in the [`Mat3`] row-vector convention (world → view).
///
/// `None` for an empty hit (the C# only gets here after a real hit).
pub fn target_rotation(hit: &HitData) -> Option<Mat3> {
    if hit.is_none() {
        return None;
    }
    let (normal, up) = direction_for_face(hit);
    Some(math::look_at(normal, up))
}
