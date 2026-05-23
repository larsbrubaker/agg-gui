//! Pure snap engine — no widget / event dependencies.
//!
//! Implements the same two-tier algorithm as Node Designer's
//! `smart-snap.js`:
//!
//! 1. **Edge / centre alignment**.  Six snap points per rect (left,
//!    right, centre-x, top, bottom, centre-y).  For each moving edge
//!    we look at every same-axis target point within `threshold`; the
//!    smallest delta wins per axis.
//!
//! 2. **Equal-spacing**.  Run only for [`SnapMode::Move`].  Find a
//!    left + right neighbour (horizontally) or top + bottom
//!    neighbour (vertically), compute the symmetric centre, and snap
//!    if the moving rect lands inside `threshold` of that centre.
//!
//! Resize mode skips spacing detection and restricts alignment to
//! the edges the active handle actually controls (a resize from the
//! right edge can't snap the moving rect's LEFT side).
//!
//! The engine is stateless — escape-accumulator logic lives at the
//! call site (each frame's drag delta is passed in already
//! integrated by the drag handler).  That keeps `compute_snap` a
//! pure function and easy to test.

use crate::geometry::Rect;

use super::model::{ResizeEdge, SnapGuide, SnapId, SnapMode, SnapResult};

/// Snap a candidate rect against a set of stationary targets.
///
/// - `moving`: the rect produced by the raw drag (un-snapped).
/// - `moving_id`: identity used to skip self-matches inside `targets`.
/// - `targets`: every other snappable rect in the scene.
/// - `threshold`: maximum pixel distance for a snap to engage.
/// - `mode`: [`SnapMode::Move`] vs [`SnapMode::Resize`].
///
/// Returns the (possibly adjusted) rect plus visual guides.
pub fn compute_snap(
    moving: Rect,
    moving_id: SnapId,
    targets: &[(SnapId, Rect)],
    threshold: f64,
    mode: SnapMode,
) -> SnapResult {
    if targets.is_empty() || threshold <= 0.0 {
        return SnapResult {
            rect: moving,
            guides: Vec::new(),
        };
    }

    // Filter once and reuse — skipping self every iteration would
    // dominate the inner loops for large scenes.
    let neighbours: Vec<Rect> = targets
        .iter()
        .filter_map(|(id, r)| (id != &moving_id).then_some(*r))
        .collect();

    if neighbours.is_empty() {
        return SnapResult {
            rect: moving,
            guides: Vec::new(),
        };
    }

    let mut rect = moving;
    let mut guides: Vec<SnapGuide> = Vec::new();

    // ── Phase 1: edge alignment ──────────────────────────────────
    let allow_left = match mode {
        SnapMode::Move => true,
        SnapMode::Resize(e) => e.affects_left(),
    };
    let allow_right = match mode {
        SnapMode::Move => true,
        SnapMode::Resize(e) => e.affects_right(),
    };
    let allow_top = match mode {
        SnapMode::Move => true,
        SnapMode::Resize(e) => e.affects_top(),
    };
    let allow_bottom = match mode {
        SnapMode::Move => true,
        SnapMode::Resize(e) => e.affects_bottom(),
    };
    // Centre snaps only make sense when BOTH edges on that axis are
    // free — otherwise sliding the centre would have to move an edge
    // that the resize handle isn't supposed to touch.
    let allow_cx = allow_left && allow_right;
    let allow_cy = allow_top && allow_bottom;

    // X axis: collect candidate offsets, keep the smallest |delta|.
    if let Some(snap) = best_x_alignment(
        rect,
        &neighbours,
        threshold,
        allow_left,
        allow_right,
        allow_cx,
    ) {
        if matches!(mode, SnapMode::Move) {
            // Translate the whole rect — both edges move by `delta`.
            rect.x += snap.delta;
        } else if let SnapMode::Resize(e) = mode {
            apply_resize_x(&mut rect, snap.edge, snap.delta, e);
        }
        guides.push(SnapGuide::VLine {
            x: snap.x,
            y0: y_span(rect, snap.target_rect).0,
            y1: y_span(rect, snap.target_rect).1,
        });
    }

    // Y axis: same shape, swap horizontal ↔ vertical.
    if let Some(snap) = best_y_alignment(
        rect,
        &neighbours,
        threshold,
        allow_top,
        allow_bottom,
        allow_cy,
    ) {
        if matches!(mode, SnapMode::Move) {
            rect.y += snap.delta;
        } else if let SnapMode::Resize(e) = mode {
            apply_resize_y(&mut rect, snap.edge, snap.delta, e);
        }
        guides.push(SnapGuide::HLine {
            y: snap.y,
            x0: x_span(rect, snap.target_rect).0,
            x1: x_span(rect, snap.target_rect).1,
        });
    }

    // ── Phase 2: equal-spacing (Move only) ───────────────────────
    if matches!(mode, SnapMode::Move) {
        if let Some(spacing) = horizontal_equal_spacing(rect, &neighbours, threshold) {
            rect.x += spacing.delta;
            // Each helper picks the right number of dimension lines
            // for the case it matched (one for sandwich, two for a
            // chain extension showing both reference + matched gap).
            guides.extend(spacing.guides);
        }
        if let Some(spacing) = vertical_equal_spacing(rect, &neighbours, threshold) {
            rect.y += spacing.delta;
            guides.extend(spacing.guides);
        }
    }

    SnapResult { rect, guides }
}

// ── Alignment internals ──────────────────────────────────────────

/// Which edge of the MOVING rect produced the alignment match.  Used
/// when `SnapMode::Resize` so we apply `delta` to the correct edge
/// instead of translating the whole rect.
#[derive(Clone, Copy, Debug)]
enum MovingEdge {
    Left,
    Right,
    CenterX,
    Top,
    Bottom,
    CenterY,
}

#[derive(Clone, Copy, Debug)]
struct AlignmentX {
    delta: f64,
    x: f64,
    edge: MovingEdge,
    target_rect: Rect,
}

#[derive(Clone, Copy, Debug)]
struct AlignmentY {
    delta: f64,
    y: f64,
    edge: MovingEdge,
    target_rect: Rect,
}

fn best_x_alignment(
    moving: Rect,
    targets: &[Rect],
    threshold: f64,
    allow_left: bool,
    allow_right: bool,
    allow_cx: bool,
) -> Option<AlignmentX> {
    let mut best: Option<AlignmentX> = None;
    let m_left = moving.x;
    let m_right = moving.x + moving.width;
    let m_cx = moving.x + moving.width * 0.5;
    for t in targets {
        let t_left = t.x;
        let t_right = t.x + t.width;
        let t_cx = t.x + t.width * 0.5;
        let moving_edges: [(MovingEdge, f64, bool); 3] = [
            (MovingEdge::Left, m_left, allow_left),
            (MovingEdge::Right, m_right, allow_right),
            (MovingEdge::CenterX, m_cx, allow_cx),
        ];
        let target_points = [t_left, t_right, t_cx];
        for (edge, mv, enabled) in moving_edges.iter().copied() {
            if !enabled {
                continue;
            }
            for tp in target_points {
                let delta = tp - mv;
                if delta.abs() <= threshold {
                    let cand = AlignmentX {
                        delta,
                        x: tp,
                        edge,
                        target_rect: *t,
                    };
                    best = match best {
                        None => Some(cand),
                        Some(prev) if delta.abs() < prev.delta.abs() => Some(cand),
                        Some(prev) => Some(prev),
                    };
                }
            }
        }
    }
    best
}

fn best_y_alignment(
    moving: Rect,
    targets: &[Rect],
    threshold: f64,
    allow_top: bool,
    allow_bottom: bool,
    allow_cy: bool,
) -> Option<AlignmentY> {
    let mut best: Option<AlignmentY> = None;
    // Y-up: rect.y is the BOTTOM edge.  "Top" in user terms = y + height.
    let m_bottom = moving.y;
    let m_top = moving.y + moving.height;
    let m_cy = moving.y + moving.height * 0.5;
    for t in targets {
        let t_bottom = t.y;
        let t_top = t.y + t.height;
        let t_cy = t.y + t.height * 0.5;
        let moving_edges: [(MovingEdge, f64, bool); 3] = [
            (MovingEdge::Bottom, m_bottom, allow_bottom),
            (MovingEdge::Top, m_top, allow_top),
            (MovingEdge::CenterY, m_cy, allow_cy),
        ];
        let target_points = [t_bottom, t_top, t_cy];
        for (edge, mv, enabled) in moving_edges.iter().copied() {
            if !enabled {
                continue;
            }
            for tp in target_points {
                let delta = tp - mv;
                if delta.abs() <= threshold {
                    let cand = AlignmentY {
                        delta,
                        y: tp,
                        edge,
                        target_rect: *t,
                    };
                    best = match best {
                        None => Some(cand),
                        Some(prev) if delta.abs() < prev.delta.abs() => Some(cand),
                        Some(prev) => Some(prev),
                    };
                }
            }
        }
    }
    best
}

fn apply_resize_x(rect: &mut Rect, edge: MovingEdge, delta: f64, active: ResizeEdge) {
    match edge {
        MovingEdge::Left if active.affects_left() => {
            rect.x += delta;
            rect.width -= delta;
        }
        MovingEdge::Right if active.affects_right() => {
            rect.width += delta;
        }
        _ => {}
    }
}

fn apply_resize_y(rect: &mut Rect, edge: MovingEdge, delta: f64, active: ResizeEdge) {
    match edge {
        MovingEdge::Bottom if active.affects_bottom() => {
            rect.y += delta;
            rect.height -= delta;
        }
        MovingEdge::Top if active.affects_top() => {
            rect.height += delta;
        }
        _ => {}
    }
}

// ── Equal-spacing internals ──────────────────────────────────────

/// Output of a spacing-match attempt.  Returned by the helpers below
/// instead of a single delta + bbox so each kind of match can emit
/// its own set of dimension-line guides — chain extension wants TWO
/// guides (the reference gap + the matched gap), the sandwich case
/// only one (the spanning gap).
#[derive(Clone, Debug)]
struct SpacingMatch {
    delta: f64,
    guides: Vec<SnapGuide>,
}

/// Detect any equal-spacing pattern the moving rect could fit into,
/// on the horizontal axis.
///
/// Two flavours:
///
/// 1. **Sandwich** — moving has both left and right neighbours.
///    Snap to the symmetric position between them.
///
/// 2. **Reference-gap matching** — for every stationary rect `Q`
///    that has its own neighbour `P` along the same axis, the gap
///    `P→Q` is a candidate reference.  If moving has a left
///    neighbour `A`, try placing moving so `gap(A→moving) == gap(P→Q)`;
///    same for a right neighbour, mirrored.  The "chain extension"
///    case (`Q == A`, `P` = `A`'s next neighbour) falls out of this
///    naturally — and the GENERAL case picks up cross-pair patterns
///    like "two windows on the right form a gap; line up the new
///    window on the left to mirror it" that PowerPoint's smart
///    guides show.
///
/// All candidate matches inside `threshold` are evaluated; the one
/// with the smallest `|delta|` wins.
fn horizontal_equal_spacing(
    moving: Rect,
    targets: &[Rect],
    threshold: f64,
) -> Option<SpacingMatch> {
    let (left_n, right_n) = horizontal_neighbours(moving, targets);

    // Sandwich case — symmetric placement between L and R.
    if let (Some(l), Some(r)) = (left_n, right_n) {
        let lr_right = l.x + l.width;
        let rl_left = r.x;
        let total = rl_left - lr_right;
        if total > moving.width {
            let symmetric_left = lr_right + (total - moving.width) * 0.5;
            let delta = symmetric_left - moving.x;
            if delta.abs() <= threshold {
                let y = moving.y + moving.height * 0.5;
                return Some(SpacingMatch {
                    delta,
                    guides: vec![SnapGuide::HSpacing {
                        y,
                        x0: lr_right,
                        x1: rl_left,
                    }],
                });
            }
        }
    }

    // Reference-gap matching: enumerate every stationary (P, Q) pair
    // where P is Q's left neighbour, take `gap(P→Q)` as the reference,
    // and try to apply it from `left_n` (extending rightward) or
    // `right_n` (extending leftward).  Smallest |delta| wins.
    let mut best: Option<SpacingMatch> = None;
    for q in targets {
        let Some(p) = horizontal_left_neighbour_of(*q, targets, None) else {
            continue;
        };
        let ref_gap = q.x - (p.x + p.width);
        if ref_gap <= 0.0 {
            continue;
        }
        let ref_guide = SnapGuide::HSpacing {
            y: q.y + q.height * 0.5,
            x0: p.x + p.width,
            x1: q.x,
        };
        if let Some(a) = left_n {
            let want_left = a.x + a.width + ref_gap;
            let delta = want_left - moving.x;
            if delta.abs() <= threshold {
                let matched_guide = SnapGuide::HSpacing {
                    y: moving.y + moving.height * 0.5,
                    x0: a.x + a.width,
                    x1: want_left,
                };
                let cand = SpacingMatch {
                    delta,
                    guides: vec![ref_guide, matched_guide],
                };
                if best.as_ref().map_or(true, |b| delta.abs() < b.delta.abs()) {
                    best = Some(cand);
                }
            }
        }
        if let Some(a) = right_n {
            let want_right = a.x - ref_gap;
            let want_left = want_right - moving.width;
            let delta = want_left - moving.x;
            if delta.abs() <= threshold {
                let matched_guide = SnapGuide::HSpacing {
                    y: moving.y + moving.height * 0.5,
                    x0: want_right,
                    x1: a.x,
                };
                let cand = SpacingMatch {
                    delta,
                    guides: vec![ref_guide, matched_guide],
                };
                if best.as_ref().map_or(true, |b| delta.abs() < b.delta.abs()) {
                    best = Some(cand);
                }
            }
        }
    }
    best
}

/// Closest vertically-overlapping neighbour of `moving` on each side.
fn horizontal_neighbours(moving: Rect, targets: &[Rect]) -> (Option<Rect>, Option<Rect>) {
    let m_left = moving.x;
    let m_right = moving.x + moving.width;
    let m_top = moving.y + moving.height;
    let m_bottom = moving.y;
    let mut left_n: Option<Rect> = None;
    let mut right_n: Option<Rect> = None;
    for t in targets {
        let t_top = t.y + t.height;
        let t_bottom = t.y;
        if !(t_top > m_bottom && t_bottom < m_top) {
            continue;
        }
        if t.x + t.width <= m_left {
            if left_n
                .as_ref()
                .map(|l| (l.x + l.width) < (t.x + t.width))
                .unwrap_or(true)
            {
                left_n = Some(*t);
            }
        } else if t.x >= m_right {
            if right_n.as_ref().map(|r| r.x > t.x).unwrap_or(true) {
                right_n = Some(*t);
            }
        }
    }
    (left_n, right_n)
}

/// Closest vertically-overlapping neighbour of `of` whose right edge
/// lies LEFT of `of.left`.  `exclude` (typically the moving rect) is
/// filtered out so a moving rect that hasn't moved yet doesn't pose
/// as its own chain reference.
fn horizontal_left_neighbour_of(of: Rect, targets: &[Rect], exclude: Option<Rect>) -> Option<Rect> {
    let mut best: Option<Rect> = None;
    let of_top = of.y + of.height;
    let of_bottom = of.y;
    let of_left = of.x;
    for t in targets {
        if let Some(e) = exclude {
            if rect_eq(*t, e) {
                continue;
            }
        }
        if rect_eq(*t, of) {
            continue;
        }
        let t_top = t.y + t.height;
        let t_bottom = t.y;
        if !(t_top > of_bottom && t_bottom < of_top) {
            continue;
        }
        if t.x + t.width <= of_left
            && best
                .as_ref()
                .map(|b| (b.x + b.width) < (t.x + t.width))
                .unwrap_or(true)
        {
            best = Some(*t);
        }
    }
    best
}

fn rect_eq(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < 1e-9
        && (a.y - b.y).abs() < 1e-9
        && (a.width - b.width).abs() < 1e-9
        && (a.height - b.height).abs() < 1e-9
}

/// Mirror of [`horizontal_equal_spacing`] for the vertical axis.
/// Sandwich case first, then reference-gap matching across every
/// stationary (P, Q) neighbour pair on the Y axis.
fn vertical_equal_spacing(
    moving: Rect,
    targets: &[Rect],
    threshold: f64,
) -> Option<SpacingMatch> {
    let (bottom_n, top_n) = vertical_neighbours(moving, targets);

    // Sandwich case.
    if let (Some(b), Some(t)) = (bottom_n, top_n) {
        let b_top = b.y + b.height;
        let t_bottom = t.y;
        let total = t_bottom - b_top;
        if total > moving.height {
            let symmetric_bottom = b_top + (total - moving.height) * 0.5;
            let delta = symmetric_bottom - moving.y;
            if delta.abs() <= threshold {
                let x = moving.x + moving.width * 0.5;
                return Some(SpacingMatch {
                    delta,
                    guides: vec![SnapGuide::VSpacing {
                        x,
                        y0: b_top,
                        y1: t_bottom,
                    }],
                });
            }
        }
    }

    // Reference-gap matching across every stationary (P, Q) pair
    // where P is Q's bottom neighbour (Y-up: P sits BELOW Q).  Tries
    // both upward (moving above its bottom neighbour) and downward
    // (moving below its top neighbour) placements.
    let mut best: Option<SpacingMatch> = None;
    for q in targets {
        let Some(p) = vertical_bottom_neighbour_of(*q, targets, None) else {
            continue;
        };
        let ref_gap = q.y - (p.y + p.height);
        if ref_gap <= 0.0 {
            continue;
        }
        let ref_guide = SnapGuide::VSpacing {
            x: q.x + q.width * 0.5,
            y0: p.y + p.height,
            y1: q.y,
        };
        if let Some(a) = bottom_n {
            // moving sits above A; gap = moving.y - (A.y + A.height).
            // want_bottom = A.y + A.height + ref_gap.
            let want_bottom = a.y + a.height + ref_gap;
            let delta = want_bottom - moving.y;
            if delta.abs() <= threshold {
                let matched_guide = SnapGuide::VSpacing {
                    x: moving.x + moving.width * 0.5,
                    y0: a.y + a.height,
                    y1: want_bottom,
                };
                let cand = SpacingMatch {
                    delta,
                    guides: vec![ref_guide, matched_guide],
                };
                if best.as_ref().map_or(true, |b| delta.abs() < b.delta.abs()) {
                    best = Some(cand);
                }
            }
        }
        if let Some(a) = top_n {
            // moving sits below A; gap = A.y - (moving.y + moving.height).
            // want_top = A.y - ref_gap → want_y = want_top - moving.height.
            let want_top = a.y - ref_gap;
            let want_bottom = want_top - moving.height;
            let delta = want_bottom - moving.y;
            if delta.abs() <= threshold {
                let matched_guide = SnapGuide::VSpacing {
                    x: moving.x + moving.width * 0.5,
                    y0: want_top,
                    y1: a.y,
                };
                let cand = SpacingMatch {
                    delta,
                    guides: vec![ref_guide, matched_guide],
                };
                if best.as_ref().map_or(true, |b| delta.abs() < b.delta.abs()) {
                    best = Some(cand);
                }
            }
        }
    }
    best
}

fn vertical_neighbours(moving: Rect, targets: &[Rect]) -> (Option<Rect>, Option<Rect>) {
    let m_left = moving.x;
    let m_right = moving.x + moving.width;
    let m_top = moving.y + moving.height;
    let m_bottom = moving.y;
    let mut bot: Option<Rect> = None;
    let mut top: Option<Rect> = None;
    for t in targets {
        let t_left = t.x;
        let t_right = t.x + t.width;
        if !(t_right > m_left && t_left < m_right) {
            continue;
        }
        let t_top = t.y + t.height;
        let t_bottom = t.y;
        if t_top <= m_bottom {
            if bot.as_ref().map(|b| (b.y + b.height) < t_top).unwrap_or(true) {
                bot = Some(*t);
            }
        } else if t_bottom >= m_top {
            if top.as_ref().map(|tn| tn.y > t_bottom).unwrap_or(true) {
                top = Some(*t);
            }
        }
    }
    (bot, top)
}

fn vertical_bottom_neighbour_of(of: Rect, targets: &[Rect], exclude: Option<Rect>) -> Option<Rect> {
    let mut best: Option<Rect> = None;
    let of_left = of.x;
    let of_right = of.x + of.width;
    let of_bottom = of.y;
    for t in targets {
        if let Some(e) = exclude {
            if rect_eq(*t, e) {
                continue;
            }
        }
        if rect_eq(*t, of) {
            continue;
        }
        let t_left = t.x;
        let t_right = t.x + t.width;
        if !(t_right > of_left && t_left < of_right) {
            continue;
        }
        let t_top = t.y + t.height;
        if t_top <= of_bottom
            && best
                .as_ref()
                .map(|b| (b.y + b.height) < t_top)
                .unwrap_or(true)
        {
            best = Some(*t);
        }
    }
    best
}


// ── Span helpers for guide line endpoints ────────────────────────

/// Y range that spans both the moving rect and a vertical-line
/// target — guide lines run end-to-end across both so the user can
/// see what aligned.
fn y_span(a: Rect, b: Rect) -> (f64, f64) {
    let y0 = a.y.min(b.y);
    let y1 = (a.y + a.height).max(b.y + b.height);
    (y0, y1)
}

fn x_span(a: Rect, b: Rect) -> (f64, f64) {
    let x0 = a.x.min(b.x);
    let x1 = (a.x + a.width).max(b.x + b.width);
    (x0, x1)
}
