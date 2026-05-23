//! Unit tests for the snap engine.
//!
//! Cover the four engine behaviours the rest of the framework relies
//! on: edge / centre alignment, equal-spacing detection, resize-edge
//! constraint, and the threshold boundary (just-inside vs
//! just-outside must produce different results).

use crate::geometry::Rect;

use super::engine::compute_snap;
use super::model::{ResizeEdge, SnapGuide, SnapId, SnapMode};

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(x, y, w, h)
}

#[test]
fn left_edge_snaps_to_target_left_within_threshold() {
    // Moving at x=103 (3 px right of target's x=100, threshold 8).
    let moving = rect(103.0, 50.0, 60.0, 40.0);
    let target = (SnapId(2), rect(100.0, 200.0, 80.0, 40.0));
    let result = compute_snap(
        moving,
        SnapId(1),
        &[target],
        8.0,
        SnapMode::Move,
    );
    assert!((result.rect.x - 100.0).abs() < 1e-9);
    assert!(result.guides.iter().any(|g| matches!(g, SnapGuide::VLine { x, .. } if (*x - 100.0).abs() < 1e-9)));
}

#[test]
fn outside_threshold_does_not_snap() {
    // Moving 120..180 (cx 150); target 300..360 (cx 330).  Every
    // edge / centre pair is >> 8 px apart.  Engine must leave the
    // rect alone.
    let moving = rect(120.0, 50.0, 60.0, 40.0);
    let target = (SnapId(2), rect(300.0, 200.0, 60.0, 40.0));
    let result = compute_snap(moving, SnapId(1), &[target], 8.0, SnapMode::Move);
    assert_eq!(result.rect.x, 120.0);
    assert!(result.guides.is_empty());
}

#[test]
fn self_id_excluded_from_targets() {
    // The moving rect is itself in the target list (common: caller
    // walks the whole scene and includes the dragger).  Engine must
    // skip it — otherwise it would snap to itself and never move.
    let moving = rect(103.0, 50.0, 60.0, 40.0);
    let result = compute_snap(
        moving,
        SnapId(1),
        &[(SnapId(1), moving)],
        8.0,
        SnapMode::Move,
    );
    assert_eq!(result.rect.x, 103.0);
    assert!(result.guides.is_empty());
}

#[test]
fn center_x_aligns_to_target_center() {
    // Moving 100..160 (cx 130).  Pick a target where ONLY the
    // centre is in threshold — target 82..182 (cx 132): every edge
    // is >18 px from any moving edge, only the centre→centre delta
    // (+2) lands inside the 8 px window.  Engine must snap centre.
    let moving = rect(100.0, 50.0, 60.0, 40.0);
    let target = (SnapId(2), rect(82.0, 200.0, 100.0, 40.0));
    let result = compute_snap(moving, SnapId(1), &[target], 8.0, SnapMode::Move);
    let new_center = result.rect.x + result.rect.width * 0.5;
    assert!(
        (new_center - 132.0).abs() < 1e-9,
        "expected snapped centre at 132, got {new_center}"
    );
}

#[test]
fn top_edge_snaps_in_y_up_coordinates() {
    // Y-up: target's "top" = y + height.  Moving's top should snap
    // to target's top.
    let moving = rect(0.0, 47.0, 50.0, 50.0); // top at y=97
    let target = (SnapId(2), rect(200.0, 50.0, 50.0, 50.0)); // top at y=100
    let result = compute_snap(moving, SnapId(1), &[target], 8.0, SnapMode::Move);
    assert!((result.rect.y + result.rect.height - 100.0).abs() < 1e-9);
}

#[test]
fn horizontal_equal_spacing_centers_between_neighbours() {
    // Two neighbours with a 100-px gap between their facing edges:
    // L right = 100, R left = 200.  Moving rect is 40 wide — symmetric
    // placement puts its left at 100 + (100-40)/2 = 130.
    let l = (SnapId(2), rect(40.0, 50.0, 60.0, 40.0));
    let r = (SnapId(3), rect(200.0, 50.0, 60.0, 40.0));
    let moving = rect(132.0, 55.0, 40.0, 30.0); // 2 px off symmetric centre
    let result = compute_snap(moving, SnapId(1), &[l, r], 8.0, SnapMode::Move);
    assert!(
        (result.rect.x - 130.0).abs() < 1e-9,
        "expected symmetric left at 130, got {}",
        result.rect.x
    );
    assert!(
        result.guides.iter().any(|g| matches!(g, SnapGuide::HSpacing { .. })),
        "spacing snap must emit an HSpacing guide"
    );
}

#[test]
fn vertical_equal_spacing_centers_between_neighbours() {
    let bot = (SnapId(2), rect(50.0, 40.0, 60.0, 40.0)); // top at 80
    let top = (SnapId(3), rect(50.0, 200.0, 60.0, 40.0)); // bottom at 200
    // Gap 80..200 = 120; moving 30 tall → symmetric bottom = 80 + 45 = 125.
    let moving = rect(55.0, 127.0, 60.0, 30.0); // 2 px off
    let result = compute_snap(moving, SnapId(1), &[bot, top], 8.0, SnapMode::Move);
    assert!(
        (result.rect.y - 125.0).abs() < 1e-9,
        "expected symmetric bottom at 125, got {}",
        result.rect.y
    );
    assert!(
        result.guides.iter().any(|g| matches!(g, SnapGuide::VSpacing { .. })),
        "vertical spacing snap must emit a VSpacing guide"
    );
}

#[test]
fn resize_east_only_snaps_right_edge() {
    // Same moving + target layout as the left-edge test, but in
    // resize-East mode the LEFT edge must NOT be allowed to snap.
    // The moving rect's right edge is at 163; target right is 180 —
    // 17 px away, > threshold 8 → no resize snap.  Result rect must
    // be unchanged.
    let moving = rect(103.0, 50.0, 60.0, 40.0);
    let target = (SnapId(2), rect(100.0, 200.0, 80.0, 40.0));
    let result = compute_snap(
        moving,
        SnapId(1),
        &[target],
        8.0,
        SnapMode::Resize(ResizeEdge::East),
    );
    assert_eq!(
        result.rect, moving,
        "resize-East must NOT snap the left edge even when in range"
    );
}

#[test]
fn resize_east_snaps_right_edge_to_target_right() {
    // Moving right at 178, target right at 180 — within 8 px.
    // Resize-East should grow the rect by 2 px (right edge moves to
    // 180, left edge stays put).
    let moving = rect(100.0, 50.0, 78.0, 40.0);
    let target = (SnapId(2), rect(40.0, 200.0, 140.0, 40.0));
    let result = compute_snap(
        moving,
        SnapId(1),
        &[target],
        8.0,
        SnapMode::Resize(ResizeEdge::East),
    );
    assert!(
        (result.rect.x - 100.0).abs() < 1e-9,
        "left edge must stay put under resize-East"
    );
    assert!(
        (result.rect.x + result.rect.width - 180.0).abs() < 1e-9,
        "right edge must snap to target's right edge"
    );
}

#[test]
fn resize_suppresses_equal_spacing() {
    // Two neighbours that would otherwise produce a spacing snap.
    // Resize mode must NOT engage equal-spacing logic.
    let l = (SnapId(2), rect(40.0, 50.0, 60.0, 40.0));
    let r = (SnapId(3), rect(200.0, 50.0, 60.0, 40.0));
    let moving = rect(132.0, 55.0, 40.0, 30.0);
    let result = compute_snap(
        moving,
        SnapId(1),
        &[l, r],
        8.0,
        SnapMode::Resize(ResizeEdge::East),
    );
    assert!(
        !result.guides.iter().any(|g| matches!(g, SnapGuide::HSpacing { .. })),
        "spacing detection must not fire in resize mode"
    );
}

#[test]
fn empty_targets_returns_input_rect_unchanged() {
    let moving = rect(10.0, 20.0, 30.0, 40.0);
    let result = compute_snap(moving, SnapId(1), &[], 8.0, SnapMode::Move);
    assert_eq!(result.rect, moving);
    assert!(result.guides.is_empty());
}

#[test]
fn smallest_delta_wins_when_multiple_targets_in_threshold() {
    // Two targets each within threshold; the closer one (delta 1)
    // must win over the farther (delta 5).
    let moving = rect(103.0, 50.0, 60.0, 40.0);
    let far = (SnapId(2), rect(98.0, 200.0, 60.0, 40.0)); // left=98, delta=-5
    let near = (SnapId(3), rect(104.0, 200.0, 60.0, 40.0)); // left=104, delta=+1
    let result = compute_snap(moving, SnapId(1), &[far, near], 8.0, SnapMode::Move);
    assert!((result.rect.x - 104.0).abs() < 1e-9);
}

#[test]
fn horizontal_chain_extension_matches_reference_gap_rightward() {
    // Three rects in a row: B (leftmost) ── gap ── A (middle) ── ?
    // The dragged "moving" rect sits to the right of A; the engine
    // should snap so the gap A→moving matches the reference gap B→A.
    //
    // Layout:
    //   B = x∈[0..60],   width 60
    //   A = x∈[100..160], width 60   → reference gap = 100−60 = 40
    //   moving = approx x≈200, width 40 → want moving.left = 160+40 = 200
    let b = (SnapId(2), rect(0.0, 50.0, 60.0, 40.0));
    let a = (SnapId(3), rect(100.0, 50.0, 60.0, 40.0));
    // Drop moving 3 px off the symmetric spot so the engine has work
    // to do.
    let moving = rect(203.0, 55.0, 40.0, 30.0);
    let result = compute_snap(moving, SnapId(1), &[b, a], 8.0, SnapMode::Move);
    assert!(
        (result.rect.x - 200.0).abs() < 1e-9,
        "chain extension must snap moving.left to A.right + (A.left - B.right); got {}",
        result.rect.x
    );
    let spacing_guides = result
        .guides
        .iter()
        .filter(|g| matches!(g, SnapGuide::HSpacing { .. }))
        .count();
    assert_eq!(
        spacing_guides, 2,
        "chain extension must emit TWO HSpacing guides (reference + matched), got {spacing_guides}"
    );
}

#[test]
fn horizontal_chain_extension_matches_reference_gap_leftward() {
    // Mirror of the above — moving sits to the LEFT of a pair.
    let a = (SnapId(2), rect(100.0, 50.0, 60.0, 40.0));
    let b = (SnapId(3), rect(200.0, 50.0, 60.0, 40.0));
    // Reference gap b−a = 200−160 = 40.  Want moving.right at
    // 100−40 = 60 → moving.left at 60 − moving.width.
    let moving = rect(15.0, 55.0, 40.0, 30.0); // 3 px off
    let result = compute_snap(moving, SnapId(1), &[a, b], 8.0, SnapMode::Move);
    assert!(
        ((result.rect.x + result.rect.width) - 60.0).abs() < 1e-9,
        "leftward chain extension must snap moving.right to A.left - ref_gap; got {}",
        result.rect.x + result.rect.width
    );
}

#[test]
fn vertical_chain_extension_matches_reference_gap_upward() {
    // Three rects stacked vertically (Y-up): B (lowest) ── gap ── A
    // (middle) ── ? → moving (top).  Engine snaps so gap A→moving
    // matches gap B→A.
    let b = (SnapId(2), rect(50.0, 0.0, 60.0, 40.0)); // top of B = 40
    let a = (SnapId(3), rect(50.0, 80.0, 60.0, 40.0)); // top of A = 120 ; gap B→A = 80−40 = 40
    // Moving 30 tall — want moving.bottom = 120+40 = 160.
    let moving = rect(55.0, 163.0, 50.0, 30.0); // 3 px off
    let result = compute_snap(moving, SnapId(1), &[b, a], 8.0, SnapMode::Move);
    assert!(
        (result.rect.y - 160.0).abs() < 1e-9,
        "upward chain extension must snap moving.bottom to A.top + ref_gap; got {}",
        result.rect.y
    );
}

#[test]
fn vertical_cross_pair_reference_gap_matches() {
    // User-reported scenario:
    //   - LEFT-top      (stationary, top-left)
    //   - RIGHT-top     (stationary, top-right)
    //   - RIGHT-bottom  (stationary, below RIGHT-top with a gap)
    //   - moving        (LEFT-bottom, dragged)
    //
    // Reference gap = gap(RIGHT-top.bottom → RIGHT-bottom.top) = 100.
    // Moving has top neighbour LEFT-top; engine must place moving so
    // gap(LEFT-top.bottom → moving.top) == 100, even though the
    // reference pair lives in a DIFFERENT column.
    //
    // Y-up coords:
    //   LEFT-top     y∈[400..500]  (height 100, top at 500)
    //   RIGHT-top    y∈[400..500]
    //   RIGHT-bottom y∈[200..300]  → ref gap = 400−300 = 100
    //   want moving.top = LEFT-top.bottom − ref_gap = 400 − 100 = 300
    //   moving height = 100 → want moving.y = 200
    let left_top = (SnapId(2), rect(0.0, 400.0, 200.0, 100.0));
    let right_top = (SnapId(3), rect(300.0, 400.0, 200.0, 100.0));
    let right_bottom = (SnapId(4), rect(300.0, 200.0, 200.0, 100.0));
    let moving = rect(0.0, 203.0, 200.0, 100.0); // 3 px off the target
    let result = compute_snap(
        moving,
        SnapId(1),
        &[left_top, right_top, right_bottom],
        8.0,
        SnapMode::Move,
    );
    assert!(
        (result.rect.y - 200.0).abs() < 1e-9,
        "cross-pair reference: expected moving.y == 200, got {}",
        result.rect.y
    );
    let spacing_guides = result
        .guides
        .iter()
        .filter(|g| matches!(g, SnapGuide::VSpacing { .. }))
        .count();
    assert_eq!(
        spacing_guides, 2,
        "cross-pair match must emit reference + matched VSpacing guides, got {spacing_guides}"
    );
}

#[test]
fn threshold_zero_disables_snapping() {
    let moving = rect(103.0, 50.0, 60.0, 40.0);
    let target = (SnapId(2), rect(100.0, 200.0, 80.0, 40.0));
    let result = compute_snap(moving, SnapId(1), &[target], 0.0, SnapMode::Move);
    assert_eq!(result.rect, moving);
    assert!(result.guides.is_empty());
}
