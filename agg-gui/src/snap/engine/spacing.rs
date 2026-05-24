use crate::geometry::Rect;

use super::super::model::{ResizeEdge, SnapGuide};

#[derive(Clone, Debug)]
pub(in crate::snap) struct SpacingMatch {
    pub(in crate::snap) delta: f64,
    pub(in crate::snap) guides: Vec<SnapGuide>,
}

/// Detect any equal-spacing pattern the moving rect could fit into on the X axis.
pub(in crate::snap) fn horizontal_equal_spacing(
    moving: Rect,
    targets: &[Rect],
    threshold: f64,
) -> Option<SpacingMatch> {
    let (left_n, right_n) = horizontal_neighbours(moving, targets);

    if let (Some(l), Some(r)) = (left_n, right_n) {
        let lr_right = l.x + l.width;
        let rl_left = r.x;
        let total = rl_left - lr_right;
        if total > moving.width {
            let symmetric_left = lr_right + (total - moving.width) * 0.5;
            let symmetric_right = symmetric_left + moving.width;
            let delta = symmetric_left - moving.x;
            if delta.abs() <= threshold {
                let y = moving.y + moving.height * 0.5;
                return Some(SpacingMatch {
                    delta,
                    guides: vec![
                        SnapGuide::HSpacing {
                            y,
                            x0: lr_right,
                            x1: symmetric_left,
                        },
                        SnapGuide::HSpacing {
                            y,
                            x0: symmetric_right,
                            x1: rl_left,
                        },
                    ],
                });
            }
        }
    }

    for &qi in targets_sorted_by_distance(moving, targets).iter() {
        let q = targets[qi];
        let Some(p) = horizontal_left_neighbour_of(q, targets, None) else {
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
        let mut best_for_q: Option<SpacingMatch> = None;
        let ref_x0 = p.x + p.width;
        let ref_x1 = q.x;
        if let Some(a) = left_n {
            let want_left = a.x + a.width + ref_gap;
            let delta = want_left - moving.x;
            let matched_x0 = a.x + a.width;
            let matched_x1 = want_left;
            if delta.abs() <= threshold && !range_eq(ref_x0, ref_x1, matched_x0, matched_x1) {
                best_for_q = Some(SpacingMatch {
                    delta,
                    guides: vec![
                        ref_guide,
                        SnapGuide::HSpacing {
                            y: moving.y + moving.height * 0.5,
                            x0: matched_x0,
                            x1: matched_x1,
                        },
                    ],
                });
            }
        }
        if let Some(a) = right_n {
            let want_right = a.x - ref_gap;
            let want_left = want_right - moving.width;
            let delta = want_left - moving.x;
            let matched_x0 = want_right;
            let matched_x1 = a.x;
            if delta.abs() <= threshold
                && !range_eq(ref_x0, ref_x1, matched_x0, matched_x1)
                && best_for_q
                    .as_ref()
                    .map_or(true, |b| delta.abs() < b.delta.abs())
            {
                best_for_q = Some(SpacingMatch {
                    delta,
                    guides: vec![
                        ref_guide,
                        SnapGuide::HSpacing {
                            y: moving.y + moving.height * 0.5,
                            x0: matched_x0,
                            x1: matched_x1,
                        },
                    ],
                });
            }
        }
        if best_for_q.is_some() {
            return best_for_q;
        }
    }
    None
}

pub(super) fn horizontal_resize_spacing(
    moving: Rect,
    targets: &[Rect],
    threshold: f64,
    edge: ResizeEdge,
) -> Option<SpacingMatch> {
    let (left_n, right_n) = horizontal_neighbours(moving, targets);
    let moving_cy = moving.y + moving.height * 0.5;
    let m_left = moving.x;
    let m_right = moving.x + moving.width;
    for &qi in targets_sorted_by_distance(moving, targets).iter() {
        let q = targets[qi];
        let Some(p) = horizontal_left_neighbour_of(q, targets, None) else {
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
        if edge.affects_right() {
            if let Some(r) = right_n {
                let want_right = r.x - ref_gap;
                let delta = want_right - m_right;
                if delta.abs() <= threshold && want_right > m_left {
                    let matched_x0 = want_right;
                    let matched_x1 = r.x;
                    if !range_eq(p.x + p.width, q.x, matched_x0, matched_x1) {
                        return Some(SpacingMatch {
                            delta,
                            guides: vec![
                                ref_guide,
                                SnapGuide::HSpacing {
                                    y: moving_cy,
                                    x0: matched_x0,
                                    x1: matched_x1,
                                },
                            ],
                        });
                    }
                }
            }
        }
        if edge.affects_left() {
            if let Some(l) = left_n {
                let want_left = l.x + l.width + ref_gap;
                let delta = want_left - m_left;
                if delta.abs() <= threshold && want_left < m_right {
                    let matched_x0 = l.x + l.width;
                    let matched_x1 = want_left;
                    if !range_eq(p.x + p.width, q.x, matched_x0, matched_x1) {
                        return Some(SpacingMatch {
                            delta,
                            guides: vec![
                                ref_guide,
                                SnapGuide::HSpacing {
                                    y: moving_cy,
                                    x0: matched_x0,
                                    x1: matched_x1,
                                },
                            ],
                        });
                    }
                }
            }
        }
    }
    None
}

/// Mirror of [`horizontal_equal_spacing`] for the Y axis.
pub(in crate::snap) fn vertical_equal_spacing(
    moving: Rect,
    targets: &[Rect],
    threshold: f64,
) -> Option<SpacingMatch> {
    let (bottom_n, top_n) = vertical_neighbours(moving, targets);

    if let (Some(b), Some(t)) = (bottom_n, top_n) {
        let b_top = b.y + b.height;
        let t_bottom = t.y;
        let total = t_bottom - b_top;
        if total > moving.height {
            let symmetric_bottom = b_top + (total - moving.height) * 0.5;
            let symmetric_top = symmetric_bottom + moving.height;
            let delta = symmetric_bottom - moving.y;
            if delta.abs() <= threshold {
                let x = moving.x + moving.width * 0.5;
                return Some(SpacingMatch {
                    delta,
                    guides: vec![
                        SnapGuide::VSpacing {
                            x,
                            y0: b_top,
                            y1: symmetric_bottom,
                        },
                        SnapGuide::VSpacing {
                            x,
                            y0: symmetric_top,
                            y1: t_bottom,
                        },
                    ],
                });
            }
        }
    }

    for &qi in targets_sorted_by_distance(moving, targets).iter() {
        let q = targets[qi];
        let Some(p) = vertical_bottom_neighbour_of(q, targets, None) else {
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
        let mut best_for_q: Option<SpacingMatch> = None;
        let ref_y0 = p.y + p.height;
        let ref_y1 = q.y;
        let ref_x = q.x + q.width * 0.5;
        let moving_x = moving.x + moving.width * 0.5;
        let same_column = (ref_x - moving_x).abs() < 1e-6;
        if let Some(a) = bottom_n {
            let want_bottom = a.y + a.height + ref_gap;
            let delta = want_bottom - moving.y;
            let matched_y0 = a.y + a.height;
            let matched_y1 = want_bottom;
            let degenerate = same_column && range_eq(ref_y0, ref_y1, matched_y0, matched_y1);
            if delta.abs() <= threshold && !degenerate {
                best_for_q = Some(SpacingMatch {
                    delta,
                    guides: vec![
                        ref_guide,
                        SnapGuide::VSpacing {
                            x: moving_x,
                            y0: matched_y0,
                            y1: matched_y1,
                        },
                    ],
                });
            }
        }
        if let Some(a) = top_n {
            let want_top = a.y - ref_gap;
            let want_bottom = want_top - moving.height;
            let delta = want_bottom - moving.y;
            let matched_y0 = want_top;
            let matched_y1 = a.y;
            let degenerate = same_column && range_eq(ref_y0, ref_y1, matched_y0, matched_y1);
            if delta.abs() <= threshold
                && !degenerate
                && best_for_q
                    .as_ref()
                    .map_or(true, |b| delta.abs() < b.delta.abs())
            {
                best_for_q = Some(SpacingMatch {
                    delta,
                    guides: vec![
                        ref_guide,
                        SnapGuide::VSpacing {
                            x: moving_x,
                            y0: matched_y0,
                            y1: matched_y1,
                        },
                    ],
                });
            }
        }
        if best_for_q.is_some() {
            return best_for_q;
        }
    }
    None
}

pub(super) fn vertical_resize_spacing(
    moving: Rect,
    targets: &[Rect],
    threshold: f64,
    edge: ResizeEdge,
) -> Option<SpacingMatch> {
    let (bottom_n, top_n) = vertical_neighbours(moving, targets);
    let moving_cx = moving.x + moving.width * 0.5;
    let m_bottom = moving.y;
    let m_top = moving.y + moving.height;
    for &qi in targets_sorted_by_distance(moving, targets).iter() {
        let q = targets[qi];
        let Some(p) = vertical_bottom_neighbour_of(q, targets, None) else {
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
        if edge.affects_top() {
            if let Some(t) = top_n {
                let want_top = t.y - ref_gap;
                let delta = want_top - m_top;
                if delta.abs() <= threshold && want_top > m_bottom {
                    let matched_y0 = want_top;
                    let matched_y1 = t.y;
                    let ref_x = q.x + q.width * 0.5;
                    let same_column = (ref_x - moving_cx).abs() < 1e-6;
                    let degenerate =
                        same_column && range_eq(p.y + p.height, q.y, matched_y0, matched_y1);
                    if !degenerate {
                        return Some(SpacingMatch {
                            delta,
                            guides: vec![
                                ref_guide,
                                SnapGuide::VSpacing {
                                    x: moving_cx,
                                    y0: matched_y0,
                                    y1: matched_y1,
                                },
                            ],
                        });
                    }
                }
            }
        }
        if edge.affects_bottom() {
            if let Some(b) = bottom_n {
                let want_bottom = b.y + b.height + ref_gap;
                let delta = want_bottom - m_bottom;
                if delta.abs() <= threshold && want_bottom < m_top {
                    let matched_y0 = b.y + b.height;
                    let matched_y1 = want_bottom;
                    let ref_x = q.x + q.width * 0.5;
                    let same_column = (ref_x - moving_cx).abs() < 1e-6;
                    let degenerate =
                        same_column && range_eq(p.y + p.height, q.y, matched_y0, matched_y1);
                    if !degenerate {
                        return Some(SpacingMatch {
                            delta,
                            guides: vec![
                                ref_guide,
                                SnapGuide::VSpacing {
                                    x: moving_cx,
                                    y0: matched_y0,
                                    y1: matched_y1,
                                },
                            ],
                        });
                    }
                }
            }
        }
    }
    None
}

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
            if bot
                .as_ref()
                .map(|b| (b.y + b.height) < t_top)
                .unwrap_or(true)
            {
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

fn targets_sorted_by_distance(moving: Rect, targets: &[Rect]) -> Vec<usize> {
    let m_cx = moving.x + moving.width * 0.5;
    let m_cy = moving.y + moving.height * 0.5;
    let mut idx: Vec<usize> = (0..targets.len()).collect();
    idx.sort_by(|&a, &b| {
        let da = sq_center_distance(targets[a], m_cx, m_cy);
        let db = sq_center_distance(targets[b], m_cx, m_cy);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });
    idx
}

fn sq_center_distance(r: Rect, cx: f64, cy: f64) -> f64 {
    let rcx = r.x + r.width * 0.5;
    let rcy = r.y + r.height * 0.5;
    let dx = rcx - cx;
    let dy = rcy - cy;
    dx * dx + dy * dy
}

fn range_eq(ref_x0: f64, ref_x1: f64, m_x0: f64, m_x1: f64) -> bool {
    (ref_x0 - m_x0).abs() < 1e-6 && (ref_x1 - m_x1).abs() < 1e-6
}

fn rect_eq(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < 1e-9
        && (a.y - b.y).abs() < 1e-9
        && (a.width - b.width).abs() < 1e-9
        && (a.height - b.height).abs() < 1e-9
}
