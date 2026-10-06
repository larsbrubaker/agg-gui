//! Tests for [`super`] (`hit_test.rs`): the C# `GetHitData` table, the
//! orientation targets of `GetDirectionForFace`, and agreement between the
//! connection table and the texture placement in `geometry.rs`.
//!
//! Expected values are worked by hand from `TumbleCubeControl.cs` (MatterCAD
//! has no unit test for these); `rust_only_*` tests check invariants of the
//! port rather than C# values.

use super::*;
use crate::widgets::tumble_cube::geometry::FACE_FRAMES;
use crate::widgets::tumble_cube::math::{self, transform};
use crate::widgets::tumble_cube::orient::{direction_for_face, target_rotation};

const TOP: usize = 0;
const LEFT: usize = 1;
const RIGHT: usize = 2;
const BOTTOM: usize = 3;
const BACK: usize = 4;
const FRONT: usize = 5;

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
}

#[test]
fn face_centres_hit_tile_4() {
    let cases = [
        ([0.0, 0.0, 2.0], TOP),
        ([-2.0, 0.0, 0.0], LEFT),
        ([2.0, 0.0, 0.0], RIGHT),
        ([0.0, 0.0, -2.0], BOTTOM),
        ([0.0, 2.0, 0.0], BACK),
        ([0.0, -2.0, 0.0], FRONT),
    ];
    for (p, face) in cases {
        assert_eq!(get_hit_data(p), HitData::new(face, 4, &[]), "{p:?}");
    }
}

#[test]
fn top_front_left_corner_matches_csharp_table() {
    // Top's left is Left(1), bottom is Front(5): branch "left, bottom" →
    // HitData(0, 0, 1, Left.Tile(0, 5) = 8, 5, Front.Tile(0, 1) = 6).
    let hit = get_hit_data([-1.5, -1.5, 2.0]);
    assert_eq!(hit.face_index, [TOP as i32, LEFT as i32, FRONT as i32]);
    assert_eq!(hit.tile_index, [0, 8, 6]);
}

#[test]
fn top_back_edge_matches_csharp_table() {
    // Top's top neighbour is Back(4) → HitData(0, 7, 4, Back.Tile(0) = 7).
    let hit = get_hit_data([0.0, 1.5, 2.0]);
    assert_eq!(hit, HitData::new(TOP, 7, &[(BACK, 7)]));
}

#[test]
fn front_right_edge_matches_csharp_table() {
    // Front's right is Right(2) → HitData(5, 5, 2, Right.Tile(5) = 3).
    let hit = get_hit_data([1.5, -2.0, 0.0]);
    assert_eq!(hit, HitData::new(FRONT, 5, &[(RIGHT, 3)]));
}

#[test]
fn point_on_no_face_falls_back_to_top_centre() {
    assert_eq!(get_hit_data([0.0, 0.0, 0.0]), HitData::new(TOP, 4, &[]));
}

#[test]
fn ray_down_the_z_axis_hits_top() {
    let p = intersect_cube([0.0, 0.0, 10.0], [0.0, 0.0, -1.0]).unwrap();
    assert!(close(p, [0.0, 0.0, 2.0]));
    assert!(intersect_cube([0.0, 0.0, 10.0], [0.0, 0.0, 1.0]).is_none());
    assert!(intersect_cube([5.0, 0.0, 10.0], [0.0, 0.0, -1.0]).is_none());
}

/// Tile (3x3, from the bottom-left) a texture coordinate falls in, with the
/// same quarter / half / quarter split as `GetHitData`'s `> 1` threshold.
fn tile_of(u: f64, v: f64) -> i32 {
    let band = |t: f64| {
        if t < 0.25 {
            0
        } else if t > 0.75 {
            2
        } else {
            1
        }
    };
    band(v) * 3 + band(u)
}

#[test]
fn rust_only_texture_placement_agrees_with_connection_table() {
    // For every face and tile: a point in that tile must hit (face, tile)
    // first, and each neighbour the C# names must see the same point in
    // the tile it reports — otherwise the hover highlight would light the
    // wrong part of a neighbouring face.
    for (face, frame) in FACE_FRAMES.iter().enumerate() {
        for tile in 0..9 {
            let u = [0.1, 0.5, 0.9][(tile % 3) as usize];
            let v = [0.1, 0.5, 0.9][(tile / 3) as usize];
            let p = math::add(
                frame.origin,
                math::add(math::scale(frame.u, u * 4.0), math::scale(frame.v, v * 4.0)),
            );
            let hit = get_hit_data(p);
            assert_eq!(
                (hit.face_index[0], hit.tile_index[0]),
                (face as i32, tile),
                "face {face} tile {tile}"
            );
            for (n, n_tile) in hit.pairs().skip(1) {
                let (nu, nv) = FACE_FRAMES[n].uv(p);
                assert_eq!(
                    tile_of(nu, nv),
                    n_tile,
                    "face {face} tile {tile} neighbour {n}"
                );
            }
        }
    }
}

#[test]
fn rust_only_face_frames_are_right_handed_and_outward() {
    for f in FACE_FRAMES.iter() {
        assert!(close(math::cross(f.u, f.v), f.normal), "{f:?}");
        let centre = math::add(f.origin, math::scale(math::add(f.u, f.v), 2.0));
        assert!(close(centre, math::scale(f.normal, 2.0)), "{f:?}");
    }
}

#[test]
fn top_click_looks_down_with_y_up() {
    let r = target_rotation(&HitData::new(TOP, 4, &[])).unwrap();
    assert!(close(transform([0.0, 0.0, -1.0], &r), [0.0, 0.0, -1.0]));
    assert!(close(transform([0.0, 1.0, 0.0], &r), [0.0, 1.0, 0.0]));
    assert!(close(transform([1.0, 0.0, 0.0], &r), [1.0, 0.0, 0.0]));
}

#[test]
fn front_click_looks_along_plus_y_with_z_up() {
    let r = target_rotation(&HitData::new(FRONT, 4, &[])).unwrap();
    // Forward (view -Z) is world +Y; world +Z is up; world +X is right.
    assert!(close(transform([0.0, 1.0, 0.0], &r), [0.0, 0.0, -1.0]));
    assert!(close(transform([0.0, 0.0, 1.0], &r), [0.0, 1.0, 0.0]));
    assert!(close(transform([1.0, 0.0, 0.0], &r), [1.0, 0.0, 0.0]));
}

#[test]
fn every_face_click_looks_at_that_face() {
    let looks = [
        (TOP, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        (LEFT, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        (RIGHT, [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        (BOTTOM, [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
        (BACK, [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
        (FRONT, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ];
    for (face, normal, up) in looks {
        let (n, u) = direction_for_face(&HitData::new(face, 4, &[]));
        assert!(close(n, normal) && close(u, up), "face {face}: {n:?} {u:?}");
        let r = target_rotation(&HitData::new(face, 4, &[])).unwrap();
        assert!(
            close(transform(normal, &r), [0.0, 0.0, -1.0]),
            "face {face}"
        );
    }
}

#[test]
fn top_edge_click_averages_normals_and_uses_z_up() {
    // Top tile 1 / Front tile 7: normal = (-Z + Y) / 2, and because the
    // first tile is not the centre the up vector is +Z.
    let hit = get_hit_data([0.0, -1.5, 2.0]);
    assert_eq!(hit, HitData::new(TOP, 1, &[(FRONT, 7)]));
    let (n, u) = direction_for_face(&hit);
    assert!(close(n, [0.0, 0.5, -0.5]));
    assert!(close(u, [0.0, 0.0, 1.0]));
}

#[test]
fn corner_click_divides_by_three() {
    let hit = get_hit_data([1.5, 1.5, 2.0]);
    assert_eq!(hit.pairs().count(), 3);
    let (n, _) = direction_for_face(&hit);
    // Top (-Z) + Right (-X) + Back (-Y), over three.
    assert!(close(n, [-1.0 / 3.0, -1.0 / 3.0, -1.0 / 3.0]));
}
