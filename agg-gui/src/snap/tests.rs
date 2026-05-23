//! Unit tests for the snap engine.
//!
//! Cover the four engine behaviours the rest of the framework relies
//! on: edge / centre alignment, equal-spacing detection, resize-edge
//! constraint, and the threshold boundary (just-inside vs
//! just-outside must produce different results).

use crate::geometry::Rect;

use super::engine::{compute_snap, horizontal_equal_spacing, vertical_equal_spacing};
use super::model::{ResizeEdge, SnapGuide, SnapId, SnapMode};

/// Helper used by spacing tests that need to assert the engine's
/// equal-gap behaviour in isolation.  Goes straight at the spacing
/// helpers rather than `compute_snap` so the assertions aren't
/// disturbed by edge alignment incidentally engaging in the test
/// scene (per the "edge suppresses spacing" precedence rule in
/// `compute_snap`).
fn rects_only(targets: &[(SnapId, Rect)]) -> Vec<Rect> {
    targets.iter().map(|(_, r)| *r).collect()
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(x, y, w, h)
}

#[test]
fn left_edge_snaps_to_target_left_within_threshold() {
    // Moving at x=103 (3 px right of target's x=100, threshold 8).
    let moving = rect(103.0, 50.0, 60.0, 40.0);
    let target = (SnapId(2), rect(100.0, 200.0, 80.0, 40.0));
    let result = compute_snap(moving, SnapId(1), &[target], 8.0, SnapMode::Move);
    assert!((result.rect.x - 100.0).abs() < 1e-9);
    assert!(result
        .guides
        .iter()
        .any(|g| matches!(g, SnapGuide::VLine { x, .. } if (*x - 100.0).abs() < 1e-9)));
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
    // placement puts its left at 100 + (100-40)/2 = 130.  We assert
    // directly against the spacing helper — `compute_snap` would
    // suppress spacing when an edge snap fires, which makes
    // scene-building for pure-spacing tests fiddly.  Edge precedence
    // is covered separately by `edge_alignment_suppresses_spacing_guides`.
    let l = (SnapId(2), rect(40.0, 50.0, 60.0, 40.0));
    let r = (SnapId(3), rect(200.0, 50.0, 60.0, 40.0));
    let moving = rect(132.0, 55.0, 40.0, 30.0);
    let m = horizontal_equal_spacing(moving, &rects_only(&[l, r]), 8.0)
        .expect("spacing match expected");
    assert!((moving.x + m.delta - 130.0).abs() < 1e-9);
    assert!(m
        .guides
        .iter()
        .any(|g| matches!(g, SnapGuide::HSpacing { .. })));
}

#[test]
fn vertical_equal_spacing_centers_between_neighbours() {
    // Direct spacing-helper assertion — see the H-axis sibling test
    // for why we bypass `compute_snap` here.
    let bot = (SnapId(2), rect(50.0, 40.0, 60.0, 40.0)); // top at 80
    let top = (SnapId(3), rect(50.0, 200.0, 60.0, 40.0)); // bottom at 200
    let moving = rect(55.0, 127.0, 60.0, 30.0); // 2 px off symmetric bottom 125
    let m = vertical_equal_spacing(moving, &rects_only(&[bot, top]), 8.0)
        .expect("spacing match expected");
    assert!((moving.y + m.delta - 125.0).abs() < 1e-9);
    assert!(m
        .guides
        .iter()
        .any(|g| matches!(g, SnapGuide::VSpacing { .. })));
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
fn resize_east_spacing_matches_reference_gap() {
    // Resize mode now supports equal-spacing snaps for the affected
    // edge.  Scene: chain of three on a row with a 40-px gap between
    // each.  Moving's right edge can be pulled to make the gap to
    // its right neighbour (R) match the reference gap (P→Q).
    //
    //   P:  x∈[0..60]
    //   Q:  x∈[100..160]   (gap P→Q = 40)
    //   R:  x∈[260..320]   ← moving's right neighbour
    //   moving:  x∈[180..217]   right edge dragged east; want gap
    //   moving.right → R.left to equal 40 → want_right = 220
    //   delta = +3 in threshold.
    //
    // Y values offset between rows so the move-cy doesn't trigger
    // an incidental edge snap (which would suppress spacing).
    let p = (SnapId(2), rect(0.0, 0.0, 60.0, 200.0));
    let q = (SnapId(3), rect(100.0, 0.0, 60.0, 200.0));
    let r = (SnapId(4), rect(260.0, 0.0, 60.0, 200.0));
    let moving = rect(180.0, 80.0, 37.0, 30.0); // right at 217, target 220
    let result = compute_snap(
        moving,
        SnapId(1),
        &[p, q, r],
        8.0,
        SnapMode::Resize(ResizeEdge::East),
    );
    assert!(
        ((result.rect.x + result.rect.width) - 220.0).abs() < 1e-6,
        "resize-east must snap moving.right so the gap to R matches P→Q; got right={}",
        result.rect.x + result.rect.width
    );
    assert!(
        (result.rect.x - 180.0).abs() < 1e-6,
        "resize-east must leave moving.x untouched; got x={}",
        result.rect.x
    );
    let spacing_guides = result
        .guides
        .iter()
        .filter(|g| matches!(g, SnapGuide::HSpacing { .. }))
        .count();
    assert_eq!(
        spacing_guides, 2,
        "resize spacing must emit ref + matched HSpacing guides; got {spacing_guides}"
    );
}

#[test]
fn sandwich_spacing_emits_two_flanking_guides() {
    // Regression for the user-reported visual bug: a sandwich match
    // used to emit ONE HSpacing line running L.right..R.left, which
    // visually passes through the moving rect.  It must now emit
    // TWO short guides — one for the left gap, one for the right —
    // so the indicator clearly straddles moving instead of
    // crossing it.
    let l = (SnapId(2), rect(40.0, 50.0, 60.0, 40.0));
    let r = (SnapId(3), rect(200.0, 50.0, 60.0, 40.0));
    let moving = rect(132.0, 55.0, 40.0, 30.0);
    let m = horizontal_equal_spacing(moving, &rects_only(&[l, r]), 8.0)
        .expect("sandwich match expected");
    let h_guides: Vec<(f64, f64)> = m
        .guides
        .iter()
        .filter_map(|g| match g {
            SnapGuide::HSpacing { x0, x1, .. } => Some((*x0, *x1)),
            _ => None,
        })
        .collect();
    assert_eq!(h_guides.len(), 2, "sandwich must emit two flanking guides");
    // Snapped moving.left = 130.  Expected ranges: [100..130] and
    // [170..200].
    assert!(
        h_guides
            .iter()
            .any(|(x0, x1)| (x0 - 100.0).abs() < 1e-6 && (x1 - 130.0).abs() < 1e-6),
        "expected left-gap guide 100..130; got {h_guides:?}"
    );
    assert!(
        h_guides
            .iter()
            .any(|(x0, x1)| (x0 - 170.0).abs() < 1e-6 && (x1 - 200.0).abs() < 1e-6),
        "expected right-gap guide 170..200; got {h_guides:?}"
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
    // B (leftmost) ── gap ── A (middle) ── ?
    // The dragged "moving" rect sits to the right of A; the engine
    // should snap so gap(A→moving) matches reference gap B→A.
    //   B = x∈[0..60]      → gap to A = 100−60 = 40
    //   A = x∈[100..160]   → want moving.left = 160+40 = 200
    let b = (SnapId(2), rect(0.0, 50.0, 60.0, 40.0));
    let a = (SnapId(3), rect(100.0, 50.0, 60.0, 40.0));
    let moving = rect(203.0, 55.0, 40.0, 30.0);
    let m = horizontal_equal_spacing(moving, &rects_only(&[b, a]), 8.0)
        .expect("chain extension expected");
    assert!((moving.x + m.delta - 200.0).abs() < 1e-9);
    let spacing_guides = m
        .guides
        .iter()
        .filter(|g| matches!(g, SnapGuide::HSpacing { .. }))
        .count();
    assert_eq!(spacing_guides, 2, "chain match must emit ref + matched");
}

#[test]
fn horizontal_chain_extension_matches_reference_gap_leftward() {
    // Mirror of the rightward case — moving sits to the LEFT of a
    // pair.  Reference gap B−A = 200−160 = 40.  Want moving.right at
    // 100−40 = 60.
    let a = (SnapId(2), rect(100.0, 50.0, 60.0, 40.0));
    let b = (SnapId(3), rect(200.0, 50.0, 60.0, 40.0));
    let moving = rect(15.0, 55.0, 40.0, 30.0);
    let m = horizontal_equal_spacing(moving, &rects_only(&[a, b]), 8.0)
        .expect("chain extension expected");
    let snapped_right = moving.x + m.delta + moving.width;
    assert!((snapped_right - 60.0).abs() < 1e-9);
}

#[test]
fn vertical_chain_extension_matches_reference_gap_upward() {
    // B (lowest) ── gap ── A (middle) ── ? → moving (top).
    // gap B→A = 80−40 = 40.  Want moving.bottom = 120+40 = 160.
    let b = (SnapId(2), rect(50.0, 0.0, 60.0, 40.0));
    let a = (SnapId(3), rect(50.0, 80.0, 60.0, 40.0));
    let moving = rect(55.0, 163.0, 50.0, 30.0);
    let m = vertical_equal_spacing(moving, &rects_only(&[b, a]), 8.0)
        .expect("chain extension expected");
    assert!((moving.y + m.delta - 160.0).abs() < 1e-9);
}

#[test]
fn vertical_cross_pair_reference_gap_matches() {
    // User-reported scenario:
    //   - LEFT-top      (stationary, top-left)
    //   - RIGHT-top     (stationary, top-right)
    //   - RIGHT-bottom  (stationary, below RIGHT-top with a gap)
    //   - moving        (LEFT-bottom, dragged)
    //
    // ref_gap = RIGHT-top.bottom − RIGHT-bottom.top = 400 − 300 = 100.
    // Engine places moving so LEFT-top.bottom − moving.top == 100, in
    // a DIFFERENT column from the reference pair.
    let left_top = (SnapId(2), rect(0.0, 400.0, 200.0, 100.0));
    let right_top = (SnapId(3), rect(300.0, 400.0, 200.0, 100.0));
    let right_bottom = (SnapId(4), rect(300.0, 200.0, 200.0, 100.0));
    let moving = rect(0.0, 203.0, 200.0, 100.0);
    let m = vertical_equal_spacing(
        moving,
        &rects_only(&[left_top, right_top, right_bottom]),
        8.0,
    )
    .expect("cross-pair match expected");
    assert!((moving.y + m.delta - 200.0).abs() < 1e-9);
    let spacing_guides = m
        .guides
        .iter()
        .filter(|g| matches!(g, SnapGuide::VSpacing { .. }))
        .count();
    assert_eq!(spacing_guides, 2, "cross-pair must emit ref + matched");
}

#[test]
fn closer_reference_pair_wins_when_multiple_match() {
    // Two reference gaps both fit the snap threshold — one FAR (top
    // of the canvas, y ≈ 820), one NEAR (y ≈ 70).  The spacing
    // helper must pick the near one so the user's eye tracks the
    // visible reference.
    let far_p = (SnapId(2), rect(0.0, 800.0, 60.0, 40.0));
    let far_q = (SnapId(3), rect(100.0, 800.0, 60.0, 40.0));
    let near_p = (SnapId(4), rect(0.0, 50.0, 60.0, 40.0));
    let near_q = (SnapId(5), rect(100.0, 50.0, 60.0, 40.0));
    let a = (SnapId(6), rect(200.0, 55.0, 60.0, 40.0));
    let moving = rect(303.0, 60.0, 40.0, 30.0);
    let m = horizontal_equal_spacing(moving, &rects_only(&[far_p, far_q, near_p, near_q, a]), 8.0)
        .expect("spacing match expected");
    assert!((moving.x + m.delta - 300.0).abs() < 1e-9);
    let ref_y = m
        .guides
        .iter()
        .find_map(|g| match g {
            SnapGuide::HSpacing { y, .. } => Some(*y),
            _ => None,
        })
        .expect("a reference HSpacing guide must be emitted");
    assert!(
        ref_y < 200.0,
        "guide should come from the NEAR pair (y < 200); got y={ref_y}"
    );
}

#[test]
fn degenerate_chain_extension_does_not_emit_overlapping_guides() {
    // When the only available reference pair (P, Q) collapses onto
    // moving's own neighbour relation — i.e., Q is moving's right
    // neighbour AND P is to Q's left — the leftward chain-extension
    // math produces a "matched" gap identical to the reference gap.
    // That would paint two pink dimension lines on top of the same
    // physical span (the user's screenshot bug); the engine must
    // refuse this candidate instead of returning it.
    //
    // Layout: P x∈[0..50], moving (initial somewhere left of A),
    //         A x∈[200..250] = moving's right neighbour.
    // ref_gap = 200 − 50 = 150.  want_right = 200 − 150 = 50 = P.right.
    // Matched gap would be 50..200 — identical to ref.  Should NOT
    // engage.
    let p = (SnapId(2), rect(0.0, 50.0, 50.0, 40.0));
    let a = (SnapId(3), rect(200.0, 50.0, 50.0, 40.0));
    // Place moving so the would-be want_left is within threshold,
    // but the degeneracy guard should still kick in.
    let moving = rect(13.0, 55.0, 40.0, 30.0); // want_left = 10, delta = -3
    let result = compute_snap(moving, SnapId(1), &[p, a], 8.0, SnapMode::Move);
    // Engine must NOT engage the degenerate chain extension.  Allow
    // alignment snaps (which fire on the left-edge to P.right pair)
    // but no HSpacing pair should reach the guide list.
    let spacing_guides = result
        .guides
        .iter()
        .filter(|g| matches!(g, SnapGuide::HSpacing { .. }))
        .count();
    assert_eq!(
        spacing_guides, 0,
        "degenerate chain extension must not emit HSpacing guides; got {spacing_guides}"
    );
}

#[test]
fn x_edge_does_not_suppress_y_spacing() {
    // Per-axis precedence: an X-axis edge snap suppresses HSpacing
    // only.  Vertical spacing must still engage independently if a
    // gap pattern exists on the Y axis.
    //
    // Scene:
    //   • Moving's right edge sits 3 px from `right_align.left` →
    //     X edge snap will fire (suppress H spacing).
    //   • Stationary `bot` / `top` form a vertical gap pattern that
    //     produces a V spacing match for moving — at a position
    //     where the moving rect doesn't accidentally trigger any
    //     Y-edge snap.
    //
    // Layout (all coords in Y-up):
    //   right_align  x∈[400..460]  y∈[0..600]   ← moving.right snaps to 400
    //   bot          x∈[100..160]  y∈[40..80]   ← V spacing neighbour below
    //   top          x∈[100..160]  y∈[200..240] ← V spacing neighbour above
    //                                              gap = 200−80 = 120
    //   moving 60 wide × 30 tall → symmetric_bottom = 80+45 = 125.
    let right_align = (SnapId(2), rect(400.0, 0.0, 60.0, 600.0));
    // bot / top sit in the same column as moving so they ARE
    // horizontally-overlapping V-neighbours (the spacing engine
    // requires horizontal overlap to consider a neighbour).
    let bot = (SnapId(3), rect(360.0, 40.0, 60.0, 40.0));
    let top = (SnapId(4), rect(360.0, 200.0, 60.0, 40.0));
    let moving = rect(343.0, 127.0, 60.0, 30.0); // right edge 403 → 3 px from 400; y 127 → 2 px from 125
    let result = compute_snap(
        moving,
        SnapId(1),
        &[right_align, bot, top],
        4.0,
        SnapMode::Move,
    );
    assert!(
        ((result.rect.x + result.rect.width) - 400.0).abs() < 1e-9,
        "X edge must snap moving.right to 400; got right={}",
        result.rect.x + result.rect.width
    );
    assert!(
        (result.rect.y - 125.0).abs() < 1e-9,
        "V spacing must still engage on the Y axis; got y={}",
        result.rect.y
    );
    let has_vline = result
        .guides
        .iter()
        .any(|g| matches!(g, SnapGuide::VLine { .. }));
    let has_vspacing = result
        .guides
        .iter()
        .any(|g| matches!(g, SnapGuide::VSpacing { .. }));
    let has_hspacing = result
        .guides
        .iter()
        .any(|g| matches!(g, SnapGuide::HSpacing { .. }));
    assert!(has_vline, "X edge alignment must emit a VLine guide");
    assert!(
        has_vspacing,
        "Y spacing must still emit a VSpacing guide (per-axis precedence)"
    );
    assert!(
        !has_hspacing,
        "X edge engagement must still suppress H spacing"
    );
}

#[test]
fn edge_alignment_suppresses_spacing_guides() {
    // Set up BOTH a tempting edge alignment AND a tempting equal-
    // spacing chain.  The engine must prefer the edge snap and
    // suppress the spacing guides entirely — chaining and aligning
    // simultaneously paints two competing explanations on top of
    // each other (user-reported regression).
    //
    // Scene:
    //   left_top  x∈[0..200]   y∈[400..500]   ← moving will edge-align
    //   right_top x∈[300..500] y∈[400..500]      to left_top's edges
    //   right_bot x∈[300..500] y∈[200..300]   ← reference pair for
    //                                            equal-spacing chain
    // Moving is positioned so:
    //   • its left edge is 3 px off `left_top.x = 0`  → X edge snaps
    //   • its top edge is 3 px off `left_top.y + height = 500`
    //     → Y edge snaps
    //   • AND the chain-extension math (gap right column = 100) would
    //     ALSO be in range — but must be suppressed.
    let left_top = (SnapId(2), rect(0.0, 400.0, 200.0, 100.0));
    let right_top = (SnapId(3), rect(300.0, 400.0, 200.0, 100.0));
    let right_bot = (SnapId(4), rect(300.0, 200.0, 200.0, 100.0));
    let moving = rect(3.0, 503.0, 200.0, 100.0);
    let result = compute_snap(
        moving,
        SnapId(1),
        &[left_top, right_top, right_bot],
        8.0,
        SnapMode::Move,
    );
    let has_edge = result
        .guides
        .iter()
        .any(|g| matches!(g, SnapGuide::VLine { .. } | SnapGuide::HLine { .. }));
    let has_spacing = result
        .guides
        .iter()
        .any(|g| matches!(g, SnapGuide::HSpacing { .. } | SnapGuide::VSpacing { .. }));
    assert!(has_edge, "edge alignment must engage in this scene");
    assert!(
        !has_spacing,
        "spacing guides must be suppressed when an edge snap fires"
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
