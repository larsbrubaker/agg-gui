//! Cube hit point → face / tile resolution.
//!
//! Port of MatterCAD's `ConnectedFaces`, `HitData` and
//! `TumbleCubeControl.GetHitData` (`TumbleCubeControl.cs`).  AtomArtist's
//! `tumble_cube/hit_test.rs` is a prior Rust port of the same code; this
//! one keeps the C# cube size (`PlatonicSolids.CreateCube(4, 4, 4)`, so
//! faces at ±2 and the edge/corner threshold `> 1`) instead of AtomArtist's
//! rescaled ±1 cube, so every constant reads exactly as in the C#.
//!
//! Face indices are the order the C# textures and connects the faces in:
//! `0 Top (+Z)`, `1 Left (-X)`, `2 Right (+X)`, `3 Bottom (-Z)`,
//! `4 Back (+Y)`, `5 Front (-Y)`.  Tiles are a 3x3 grid on each face
//! texture, numbered from the texture's bottom-left (see
//! [`super::face_textures::tile_rect`]):
//!
//! ```text
//!   6 7 8
//!   3 4 5
//!   0 1 2
//! ```

use super::math::Vec3;

/// Half the cube's edge length: the C# cube is 4 units on a side.
pub const CUBE_HALF_SIZE: f64 = 2.0;

/// Face labels in face-index order — the C# `TextureFace` calls.
pub const FACE_NAMES: [&str; 6] = ["Top", "Left", "Right", "Bottom", "Back", "Front"];

/// One face's neighbours — C# `ConnectedFaces`.
///
/// `axis`/`direction` say which side of which world axis the face is on;
/// `left`/`bottom`/`right`/`top` are the face indices that share the
/// texture's left/bottom/right/top edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConnectedFaces {
    pub axis: usize,
    pub direction: f64,
    pub left: usize,
    pub bottom: usize,
    pub right: usize,
    pub top: usize,
}

impl ConnectedFaces {
    const fn new(
        axis: usize,
        direction: f64,
        left: usize,
        bottom: usize,
        right: usize,
        top: usize,
    ) -> Self {
        Self {
            axis,
            direction,
            left,
            bottom,
            right,
            top,
        }
    }

    /// Tile of this face that touches `face_sharing_edge` — C# `Tile(int)`.
    pub fn tile_for_edge(&self, face_sharing_edge: usize) -> i32 {
        if face_sharing_edge == self.left {
            3
        } else if face_sharing_edge == self.bottom {
            1
        } else if face_sharing_edge == self.right {
            5
        } else if face_sharing_edge == self.top {
            7
        } else {
            4
        }
    }

    /// Tile of this face at the corner shared with faces `a` and `b` —
    /// C# `Tile(int, int)`, branch for branch.
    pub fn tile_for_corner(&self, a: usize, b: usize) -> i32 {
        if a == self.left {
            if b == self.top {
                6
            } else {
                0
            }
        } else if a == self.bottom {
            if b == self.left {
                0
            } else {
                2
            }
        } else if a == self.right {
            if b == self.top {
                8
            } else {
                2
            }
        } else if a == self.top {
            if b == self.left {
                6
            } else {
                8
            }
        } else {
            4
        }
    }
}

/// The connection table, verbatim from the six `connections.Add(...)`
/// calls in `TumbleCubeControl.EnsureTexturesBuilt`
/// (`new ConnectedFaces(axis, offset, left, bottom, right, top)`).
pub const CONNECTIONS: [ConnectedFaces; 6] = [
    ConnectedFaces::new(2, 1.0, 1, 5, 2, 4),
    ConnectedFaces::new(0, -1.0, 4, 3, 5, 0),
    ConnectedFaces::new(0, 1.0, 5, 3, 4, 0),
    ConnectedFaces::new(2, -1.0, 1, 4, 2, 5),
    ConnectedFaces::new(1, 1.0, 2, 3, 1, 0),
    ConnectedFaces::new(1, -1.0, 1, 3, 2, 0),
];

/// Up to three `(face, tile)` pairs — C# `HitData`.  A face-centre hit
/// fills slot 0, an edge hit slots 0-1, a corner hit all three; unused
/// slots hold `-1` exactly like the C# arrays.
///
/// Equality compares both arrays element-wise.  (The C# `Equals` compares
/// the `TileIndex` array *references*, so two distinct `HitData` never
/// compare equal there and the hover texture is redrawn on every mouse
/// move; comparing values is what that code intends.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitData {
    pub face_index: [i32; 3],
    pub tile_index: [i32; 3],
}

impl Default for HitData {
    fn default() -> Self {
        Self::NONE
    }
}

impl HitData {
    /// No hit — the C# `new HitData()`.
    pub const NONE: HitData = HitData {
        face_index: [-1; 3],
        tile_index: [-1; 3],
    };

    /// C# `new HitData(f0, t0, f1 = -1, t1 = -1, f2 = -1, t2 = -1)`.
    pub fn new(f0: usize, t0: i32, rest: &[(usize, i32)]) -> Self {
        let mut h = Self::NONE;
        h.face_index[0] = f0 as i32;
        h.tile_index[0] = t0;
        for (slot, (f, t)) in rest.iter().enumerate().take(2) {
            h.face_index[slot + 1] = *f as i32;
            h.tile_index[slot + 1] = *t;
        }
        h
    }

    pub fn is_none(&self) -> bool {
        self.face_index[0] < 0
    }

    /// The occupied `(face, tile)` pairs, in slot order.
    pub fn pairs(&self) -> impl Iterator<Item = (usize, i32)> + '_ {
        (0..3)
            .take_while(move |&i| self.face_index[i] >= 0)
            .map(move |i| (self.face_index[i] as usize, self.tile_index[i]))
    }
}

/// Resolve a point on the cube surface (mesh coordinates, faces at ±2) to
/// the face/edge/corner it lies in — C# `GetHitData`, same branch order.
pub fn get_hit_data(hit_position: Vec3) -> HitData {
    let c = &CONNECTIONS;
    // "Past the neighbour's threshold": the hit is within one unit of the
    // edge shared with that neighbour.
    let toward = |n: usize| hit_position[c[n].axis] * c[n].direction > 1.0;
    for (i, face) in c.iter().enumerate() {
        if (hit_position[face.axis] - face.direction * CUBE_HALF_SIZE).abs() >= 0.0001 {
            continue;
        }
        let corner = |tile: i32, a: usize, b: usize| {
            HitData::new(
                i,
                tile,
                &[
                    (a, c[a].tile_for_corner(i, b)),
                    (b, c[b].tile_for_corner(i, a)),
                ],
            )
        };
        let edge = |tile: i32, n: usize| HitData::new(i, tile, &[(n, c[n].tile_for_edge(i))]);

        if toward(face.left) {
            if toward(face.bottom) {
                return corner(0, face.left, face.bottom);
            } else if toward(face.top) {
                return corner(6, face.left, face.top);
            }
            return edge(3, face.left);
        } else if toward(face.right) {
            if toward(face.bottom) {
                return corner(2, face.right, face.bottom);
            } else if toward(face.top) {
                return corner(8, face.right, face.top);
            }
            return edge(5, face.right);
        }
        if toward(face.bottom) {
            return edge(1, face.bottom);
        } else if toward(face.top) {
            return edge(7, face.top);
        }
        return HitData::new(i, 4, &[]);
    }
    // The C# falls back to the Top face centre for a point on no face.
    HitData::new(0, 4, &[])
}

/// Closest intersection of a ray with the cube (`[-2, 2]^3`), as the C#
/// gets from the cube's BVH `GetClosestIntersection`.  Slab test; returns
/// the hit point with the struck face's coordinate snapped exactly onto
/// the face plane so [`get_hit_data`]'s `1e-4` plane test always matches.
pub fn intersect_cube(origin: Vec3, dir: Vec3) -> Option<Vec3> {
    let h = CUBE_HALF_SIZE;
    let mut t_near = f64::NEG_INFINITY;
    let mut t_far = f64::INFINITY;
    let mut near_axis = 0usize;
    for k in 0..3 {
        if dir[k].abs() < 1e-12 {
            if origin[k] < -h || origin[k] > h {
                return None;
            }
            continue;
        }
        let t1 = (-h - origin[k]) / dir[k];
        let t2 = (h - origin[k]) / dir[k];
        let (lo, hi) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
        if lo > t_near {
            t_near = lo;
            near_axis = k;
        }
        t_far = t_far.min(hi);
        if t_near > t_far {
            return None;
        }
    }
    // The ray starts outside the cube (the eye), so only a forward entry
    // counts — a ray from inside or behind is a miss.
    if t_near < 0.0 || !t_near.is_finite() {
        return None;
    }
    let mut p = [
        origin[0] + dir[0] * t_near,
        origin[1] + dir[1] * t_near,
        origin[2] + dir[2] * t_near,
    ];
    p[near_axis] = if dir[near_axis] < 0.0 { h } else { -h };
    Some(p)
}

#[cfg(test)]
#[path = "hit_test_tests.rs"]
mod tests;
