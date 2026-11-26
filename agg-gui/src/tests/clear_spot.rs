//! Where a floating card fits: `nearest_clear_spot` puts a box inside a
//! region clear of every hard obstacle, covering as little of the soft ones
//! as it can, as near its ideal spot as that allows. Hand-made cases pin the
//! squeeze between two obstacles and the tie-break; randomized cases with a
//! fixed seed check every answer against an exact scan (every whole-unit
//! position and every whole unit off the ideal spot), check that soft
//! obstacles covering the whole region off the whole-unit grid change
//! nothing, and 50 obstacles are timed.

use crate::geometry::{Point, Rect, Size};
use crate::widget::nearest_clear_spot;
use std::time::Instant;

/// The area two rectangles share, computed from their edges as a host would.
fn shared_area(a: Rect, b: Rect) -> f64 {
    let w = (a.right().min(b.right()) - a.left().max(b.left())).max(0.0);
    let h = (a.top().min(b.top()) - a.bottom().max(b.bottom())).max(0.0);
    w * h
}

fn inside(outer: Rect, inner: Rect) -> bool {
    outer.left() <= inner.left()
        && outer.bottom() <= inner.bottom()
        && inner.right() <= outer.right()
        && inner.top() <= outer.top()
}

fn distance_sq(r: Rect, ideal: Point) -> f64 {
    let dx = r.x - ideal.x;
    let dy = r.y - ideal.y;
    dx * dx + dy * dy
}

const REGION: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);
const CARD: Size = Size::new(20.0, 10.0);

#[test]
fn an_open_ideal_spot_is_kept() {
    let spot = nearest_clear_spot(REGION, &[], &[], CARD, Point::new(30.0, 40.0));
    assert_eq!(spot, Some(Rect::new(30.0, 40.0, 20.0, 10.0)));
}

#[test]
fn an_ideal_spot_past_the_region_moves_inside_it() {
    let spot = nearest_clear_spot(REGION, &[], &[], CARD, Point::new(95.0, -5.0));
    assert_eq!(spot, Some(Rect::new(80.0, 0.0, 20.0, 10.0)));
}

#[test]
fn a_blocked_ideal_spot_moves_to_the_nearest_edge_of_the_obstacle() {
    let control = Rect::new(25.0, 35.0, 30.0, 30.0);
    // Ideal at (30, 40): the card fits left of the control at x 5 (25 away),
    // right of it at 55 (25), below at y 25 (15) or above at 65 (25).
    let spot = nearest_clear_spot(REGION, &[control], &[], CARD, Point::new(30.0, 40.0));
    assert_eq!(spot, Some(Rect::new(30.0, 25.0, 20.0, 10.0)));
}

#[test]
fn a_spot_squeezed_between_two_obstacles_is_found() {
    // Two controls side by side with a 20-wide gap between x 40 and 60 below
    // y 50; above y 50 a wide bar blocks everything. The ideal spot is in
    // the bar; the nearest place is the gap's top, where the bar's grown
    // edge crosses the gap, a corner of neither obstacle.
    let left = Rect::new(0.0, 0.0, 40.0, 50.0);
    let right = Rect::new(60.0, 0.0, 40.0, 50.0);
    let bar = Rect::new(0.0, 50.0, 100.0, 50.0);
    let spot = nearest_clear_spot(
        REGION,
        &[left, right, bar],
        &[],
        CARD,
        Point::new(45.0, 70.0),
    );
    assert_eq!(spot, Some(Rect::new(40.0, 40.0, 20.0, 10.0)));
}

#[test]
fn no_spot_when_nothing_fits() {
    let all = [Rect::new(0.0, 0.0, 100.0, 100.0)];
    assert_eq!(
        nearest_clear_spot(REGION, &all, &[], CARD, Point::ORIGIN),
        None
    );
    let too_big = Size::new(101.0, 10.0);
    assert_eq!(
        nearest_clear_spot(REGION, &[], &[], too_big, Point::ORIGIN),
        None
    );
    // A gap one unit too narrow.
    let left = Rect::new(0.0, 0.0, 40.0, 100.0);
    let right = Rect::new(59.0, 0.0, 41.0, 100.0);
    assert_eq!(
        nearest_clear_spot(REGION, &[left, right], &[], CARD, Point::ORIGIN),
        None
    );
    // Exactly wide enough: touching both sides is clear.
    let right = Rect::new(60.0, 0.0, 40.0, 100.0);
    assert_eq!(
        nearest_clear_spot(REGION, &[left, right], &[], CARD, Point::ORIGIN),
        Some(Rect::new(40.0, 0.0, 20.0, 10.0))
    );
}

#[test]
fn a_box_placed_against_an_obstacle_keeps_clear_of_it_after_rounding() {
    // 10.4 - 2.3 + 2.3 comes to just over 10.4 in f64, so a box placed by
    // subtracting its width alone would reach an ulp into the wall.
    let wall = Rect::new(10.4, 0.0, 5.0, 100.0);
    let size = Size::new(2.3, 1.0);
    let spot = nearest_clear_spot(REGION, &[wall], &[], size, Point::new(9.0, 50.0))
        .expect("there is room left of the wall");
    assert_eq!(shared_area(spot, wall), 0.0);
    assert!(
        spot.right() <= 10.4 && spot.right() > 10.4 - 1e-12,
        "{spot:?} touches the wall"
    );
}

#[test]
fn soft_obstacles_are_covered_only_when_nothing_better_fits() {
    let soft = Rect::new(20.0, 30.0, 40.0, 30.0);
    let spot = nearest_clear_spot(REGION, &[], &[soft], CARD, Point::new(30.0, 40.0));
    // Just below and just above are equally near; the higher wins.
    assert_eq!(
        spot,
        Some(Rect::new(30.0, 60.0, 20.0, 10.0)),
        "just above it"
    );

    // With the region no taller than the soft obstacle, the card covers the
    // least of it at the region's top or bottom, half over its edge, and
    // the bottom is nearer.
    let band = Rect::new(0.0, 30.0, 100.0, 30.0);
    let spot = nearest_clear_spot(
        band,
        &[],
        &[Rect::new(0.0, 35.0, 100.0, 20.0)],
        CARD,
        Point::new(30.0, 38.0),
    );
    assert_eq!(spot, Some(Rect::new(30.0, 30.0, 20.0, 10.0)));
}

#[test]
fn equally_near_spots_prefer_the_higher_then_the_left() {
    // A control right on the ideal spot, as wide as the card is tall: below
    // and above are equally near, left and right farther.
    let control = Rect::new(30.0, 40.0, 20.0, 10.0);
    let spot = nearest_clear_spot(REGION, &[control], &[], CARD, Point::new(30.0, 40.0));
    assert_eq!(spot, Some(Rect::new(30.0, 50.0, 20.0, 10.0)), "above");
    // Left and right equally near, above and below blocked by a column.
    let column = Rect::new(30.0, 0.0, 20.0, 100.0);
    let spot = nearest_clear_spot(REGION, &[column], &[], CARD, Point::new(30.0, 40.0));
    assert_eq!(spot, Some(Rect::new(10.0, 40.0, 20.0, 10.0)), "left");
}

/// A fixed-seed xorshift, so every run checks the same cases.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// A whole number in `0..n`, as `f64`.
    fn below(&mut self, n: u64) -> f64 {
        // Below 2^53, so exact in f64.
        (self.next() % n) as f64
    }
    fn rect(&mut self, limit: u64, most: u64) -> Rect {
        Rect::new(
            self.below(limit),
            self.below(limit),
            1.0 + self.below(most),
            1.0 + self.below(most),
        )
    }
}

/// The positions along one axis the scan tries: every whole unit from the
/// region's start, and every whole unit off the ideal coordinate, that keep
/// a box of `extent` inside `[low, high]`, sorted.
fn scan_positions(low: f64, high: f64, extent: f64, ideal: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut v = low;
    while v + extent <= high {
        out.push(v);
        v += 1.0;
    }
    let mut v = ideal - (ideal - low).floor();
    while v + extent <= high {
        out.push(v);
        v += 1.0;
    }
    out.sort_by(f64::total_cmp);
    out.dedup();
    out
}

/// The best position for the box by scanning every whole-unit position and
/// every whole unit off the ideal spot's coordinates: least soft area
/// covered, then nearest. With whole-unit obstacles and region the best spot
/// lies among them, so the scan is exact. `None` when none fits.
fn scan(
    region: Rect,
    hard: &[Rect],
    soft: &[Rect],
    size: Size,
    ideal: Point,
) -> Option<(f64, f64)> {
    let xs = scan_positions(region.left(), region.right(), size.width, ideal.x);
    let ys = scan_positions(region.bottom(), region.top(), size.height, ideal.y);
    let mut best: Option<(f64, f64)> = None;
    for &y in &ys {
        for &x in &xs {
            let card = Rect::new(x, y, size.width, size.height);
            if hard.iter().all(|o| shared_area(card, *o) == 0.0) {
                let covered: f64 = soft.iter().map(|o| shared_area(card, *o)).sum();
                let key = (covered, distance_sq(card, ideal));
                if best.is_none_or(|b| key < b) {
                    best = Some(key);
                }
            }
        }
    }
    best
}

#[test]
fn every_answer_is_clear_inside_and_as_good_as_an_exact_scan() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut found = 0;
    for case in 0..400 {
        let region = Rect::new(
            rng.below(5),
            rng.below(5),
            8.0 + rng.below(25),
            8.0 + rng.below(25),
        );
        let size = Size::new(1.0 + rng.below(10), 1.0 + rng.below(8));
        let hard: Vec<Rect> = (0..rng.below(7) as usize)
            .map(|_| rng.rect(30, 12))
            .collect();
        let soft: Vec<Rect> = (0..rng.below(4) as usize)
            .map(|_| rng.rect(30, 15))
            .collect();
        // Half the cases want a spot off the whole-unit grid.
        let half = if case % 2 == 0 { 0.0 } else { 0.5 };
        let ideal = Point::new(rng.below(40) - 4.0 + half, rng.below(40) - 4.0 + half / 2.0);

        let spot = nearest_clear_spot(region, &hard, &soft, size, ideal);
        let scanned = scan(region, &hard, &soft, size, ideal);
        assert_eq!(
            spot.is_some(),
            scanned.is_some(),
            "case {case}: fits exactly when the scan fits"
        );
        let (Some(spot), Some((scan_covered, scan_distance))) = (spot, scanned) else {
            continue;
        };
        found += 1;
        assert_eq!((spot.width, spot.height), (size.width, size.height));
        assert!(
            inside(region, spot),
            "case {case}: {spot:?} outside {region:?}"
        );
        for o in &hard {
            assert_eq!(
                shared_area(spot, *o),
                0.0,
                "case {case}: {spot:?} covers {o:?}"
            );
        }
        let covered: f64 = soft.iter().map(|o| shared_area(spot, *o)).sum();
        assert_eq!(covered, scan_covered, "case {case}: least soft area");
        assert_eq!(
            distance_sq(spot, ideal),
            scan_distance,
            "case {case}: {spot:?} as near as the scan's best"
        );
    }
    assert!(found > 200, "most cases have a spot: {found}");
}

#[test]
fn a_soft_obstacle_over_the_whole_region_leaves_the_ideal_spot_alone() {
    // A box anywhere inside the soft rect covers the same area of it, so
    // only the distance decides, off the whole-unit grid too.
    let region = Rect::new(0.0, 0.0, 1000.0, 700.0);
    let size = Size::new(260.3, 90.7);
    let ideal = Point::new(123.456, 321.987);
    assert_eq!(
        nearest_clear_spot(region, &[], &[region], size, ideal),
        Some(Rect::new(123.456, 321.987, 260.3, 90.7))
    );
}

#[test]
fn soft_obstacles_over_the_whole_region_or_outside_it_change_nothing() {
    let mut rng = Rng(0x6A09_E667_F3BC_C908);
    let unit = |rng: &mut Rng| rng.below(1 << 20) / f64::from(1 << 20);
    for case in 0..400 {
        let region = Rect::new(
            unit(&mut rng) * 71.3,
            unit(&mut rng) * 31.7,
            200.0 + unit(&mut rng) * 137.9,
            150.0 + unit(&mut rng) * 99.1,
        );
        let size = Size::new(3.3 + unit(&mut rng) * 77.7, 1.1 + unit(&mut rng) * 53.3);
        let hard: Vec<Rect> = (0..rng.below(9) as usize)
            .map(|_| {
                Rect::new(
                    unit(&mut rng) * 333.3,
                    unit(&mut rng) * 271.1,
                    0.1 + unit(&mut rng) * 97.7,
                    0.1 + unit(&mut rng) * 79.9,
                )
            })
            .collect();
        // Soft rects reaching past the region on every side, and one wholly
        // right of it.
        let mut soft: Vec<Rect> = (0..1 + rng.below(3) as usize)
            .map(|_| {
                let (l, b) = (unit(&mut rng) * 9.7, unit(&mut rng) * 5.3);
                Rect::new(
                    region.x - l,
                    region.y - b,
                    region.width + l + unit(&mut rng) * 3.9,
                    region.height + b + unit(&mut rng) * 2.1,
                )
            })
            .collect();
        soft.push(Rect::new(region.right() + 0.7, region.y, 50.3, 40.1));
        let ideal = Point::new(unit(&mut rng) * 400.0, unit(&mut rng) * 300.0);
        assert_eq!(
            nearest_clear_spot(region, &hard, &soft, size, ideal),
            nearest_clear_spot(region, &hard, &[], size, ideal),
            "case {case}"
        );
    }
}

#[test]
fn answers_off_the_whole_unit_grid_stay_clear_and_inside() {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let unit = |rng: &mut Rng| rng.below(1 << 20) / f64::from(1 << 20);
    for case in 0..400 {
        let region = Rect::new(
            unit(&mut rng) * 7.3,
            unit(&mut rng) * 3.1,
            20.0 + unit(&mut rng) * 13.7,
            15.0 + unit(&mut rng) * 9.9,
        );
        let size = Size::new(0.3 + unit(&mut rng) * 7.7, 0.1 + unit(&mut rng) * 5.3);
        let hard: Vec<Rect> = (0..rng.below(9) as usize)
            .map(|_| {
                Rect::new(
                    unit(&mut rng) * 33.3,
                    unit(&mut rng) * 27.1,
                    0.1 + unit(&mut rng) * 9.7,
                    0.1 + unit(&mut rng) * 7.9,
                )
            })
            .collect();
        let ideal = Point::new(unit(&mut rng) * 40.0, unit(&mut rng) * 30.0);
        if let Some(spot) = nearest_clear_spot(region, &hard, &[], size, ideal) {
            assert!(
                inside(region, spot),
                "case {case}: {spot:?} outside {region:?}"
            );
            for o in &hard {
                assert_eq!(
                    shared_area(spot, *o),
                    0.0,
                    "case {case}: {spot:?} covers {o:?}"
                );
            }
        }
    }
}

#[test]
fn a_spot_is_found_among_fifty_obstacles() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let region = Rect::new(0.0, 0.0, 1600.0, 900.0);
    let hard: Vec<Rect> = (0..40).map(|_| rng.rect(1500, 120)).collect();
    let soft: Vec<Rect> = (0..10).map(|_| rng.rect(1500, 300)).collect();
    let size = Size::new(260.0, 90.0);
    let ideal = Point::new(700.0, 400.0);
    let runs = 200u32;
    let start = Instant::now();
    let mut spot = None;
    for _ in 0..runs {
        spot = std::hint::black_box(nearest_clear_spot(
            std::hint::black_box(region),
            &hard,
            &soft,
            size,
            ideal,
        ));
    }
    let each = start.elapsed() / runs;
    eprintln!("nearest_clear_spot: 40 hard + 10 soft obstacles, {each:?} per call");
    assert!(spot.is_some());
}
