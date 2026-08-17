//! Unit tests for the vector-icon types, the path-data subset parser,
//! and the global registry.
//!
//! The registry is process-global, so these tests never call
//! `clear_icons` — they register under test-only ids and assert on those
//! ids alone, which keeps them safe under the default multi-threaded
//! test runner.

use super::*;
use crate::draw_ctx::FillRule;

/// A square, as four corners, whichever spelling it arrives in.
#[test]
fn parses_the_h_v_shorthand_into_a_closed_square() {
    let c = parse_path("M0 0 H52 V52 H0 Z").expect("square parses");
    assert_eq!(c.len(), 1);
    // The trailing Z re-states the start point, which the fill closes
    // over; what matters is the four distinct corners.
    let pts = &c[0];
    assert!(pts.contains(&[0.0, 0.0]));
    assert!(pts.contains(&[52.0, 0.0]));
    assert!(pts.contains(&[52.0, 52.0]));
    assert!(pts.contains(&[0.0, 52.0]));
}

#[test]
fn relative_commands_track_the_current_point() {
    let abs = parse_path("M10 10 L20 10 L20 20 Z").expect("absolute parses");
    let rel = parse_path("m10 10 l10 0 l0 10 z").expect("relative parses");
    assert_eq!(abs, rel);
}

/// Every arc in the icon artwork is an `A` with equal radii; the parser
/// must land exactly on the stated endpoint or adjacent segments open a
/// seam.
#[test]
fn an_arc_ends_on_its_stated_endpoint_and_bulges_outward() {
    let c = parse_path("M64 42 A22 22 0 0 1 42 64 Z").expect("quarter arc parses");
    let pts = &c[0];
    assert_eq!(pts[0], [64.0, 42.0]);
    let last_arc_point = pts[pts.len() - 1];
    assert!(
        (last_arc_point[0] - 42.0).abs() < 1e-9 && (last_arc_point[1] - 64.0).abs() < 1e-9,
        "arc ended at {last_arc_point:?}"
    );
    // Sweep=1 in SVG's Y-down space curves away from the centre (42,42).
    for p in &pts[1..] {
        let d = ((p[0] - 42.0).powi(2) + (p[1] - 42.0).powi(2)).sqrt();
        assert!((d - 22.0).abs() < 1e-6, "point {p:?} is off the circle");
    }
}

/// A 90° arc must be flattened into more than a couple of chords, or a
/// 16-px icon's curves read as facets.
#[test]
fn arcs_are_flattened_finely_enough_to_read_as_curves() {
    let c = parse_path("M64 42 A22 22 0 0 1 42 64").expect("arc parses");
    assert!(
        c[0].len() >= 12,
        "a quarter arc produced only {} points",
        c[0].len()
    );
}

#[test]
fn two_subpaths_produce_two_contours() {
    let d = "M0 0 H10 V10 H0 Z M2 2 H8 V8 H2 Z";
    let c = parse_path(d).expect("two subpaths parse");
    assert_eq!(c.len(), 2);
}

/// Regression: `Z` takes no arguments, so a number after one has no
/// command to repeat. The implicit-repetition rule used to hand that
/// number back to `Z`, which consumes nothing — the parser spun forever
/// on a malformed path instead of rejecting it.
#[test]
fn a_number_after_a_closepath_is_an_error_not_an_infinite_loop() {
    assert_eq!(
        parse_path("M0 0 H10 Z 5 5"),
        Err(IconPathError::UnexpectedNumber)
    );
    assert_eq!(
        parse_path("M0 0 H10 z 5 5"),
        Err(IconPathError::UnexpectedNumber)
    );
}

/// The one SVG spelling this subset refuses: arc flags packed against
/// their neighbours. It must be rejected, not silently mis-drawn — see
/// this module's header for why the omission is deliberate.
#[test]
fn compressed_arc_flags_are_rejected_not_misdrawn() {
    // `A22 22 0 0110 10` is `large=0 sweep=1 x=10 y=10` in compressed
    // form; read as whole numbers it runs out of arguments.
    assert_eq!(
        parse_path("M0 0 A22 22 0 0110 10"),
        Err(IconPathError::MissingArgument('A'))
    );
    // The same arc, written with separators, parses.
    assert!(parse_path("M0 0 A22 22 0 0 1 10 10").is_ok());
}

/// An arc whose endpoints coincide is omitted entirely (spec F.6.2),
/// not turned into a duplicate point.
#[test]
fn a_zero_length_arc_adds_no_points() {
    let c = parse_path("M10 10 A22 22 0 0 1 10 10 L20 10").expect("parses");
    assert_eq!(c[0], vec![[10.0, 10.0], [20.0, 10.0]]);
}

#[test]
fn unsupported_and_truncated_commands_are_errors_not_panics() {
    assert_eq!(
        parse_path("M0 0 Q5 5 10 10"),
        Err(IconPathError::UnsupportedCommand('Q'))
    );
    assert_eq!(
        parse_path("M0 0 L10"),
        Err(IconPathError::MissingArgument('L'))
    );
    assert_eq!(parse_path("L10 10"), Err(IconPathError::NoCurrentPoint));
}

#[test]
fn ink_resolves_to_the_callers_colour_and_literals_pass_through() {
    let ink = Color::rgb(0.1, 0.2, 0.3);
    assert_eq!(IconColor::Ink.resolve(ink), ink);
    let red = Color::from_rgb8(0xF2, 0x0D, 0x0D);
    assert_eq!(IconColor::Literal(red).resolve(ink), red);
}

#[test]
fn registry_round_trips_an_icon() {
    let icon_in = VectorIcon::new(64.0)
        .with_svg_path("M0 0 H52 V52 H0 Z", IconColor::Ink, FillRule::NonZero)
        .expect("path parses");
    register_icon("test.registry.round_trip", icon_in.clone());

    let out = icon("test.registry.round_trip").expect("registered icon is found");
    assert_eq!(*out, icon_in);
    assert!(icon_ids()
        .iter()
        .any(|i| &**i == "test.registry.round_trip"));
    assert!(icon("test.registry.nothing_here").is_none());
}

#[test]
fn re_registering_an_id_replaces_the_icon() {
    let one = VectorIcon::new(64.0)
        .with_svg_path_nonzero("M0 0 H10 V10 H0 Z", IconColor::Ink)
        .expect("parses");
    let two = VectorIcon::new(64.0)
        .with_svg_path_nonzero("M0 0 H20 V20 H0 Z", IconColor::Ink)
        .expect("parses");
    register_icon("test.registry.replace", one);
    register_icon("test.registry.replace", two.clone());
    assert_eq!(*icon("test.registry.replace").expect("present"), two);
}

/// Recording context: enough of `DrawCtx` to see where the icon landed.
#[derive(Default)]
struct PointRecorder {
    points: Vec<[f64; 2]>,
    fills: Vec<Color>,
    rules: Vec<FillRule>,
    color: Color,
    rule: FillRule,
}

impl DrawCtx for PointRecorder {
    fn set_fill_color(&mut self, color: Color) {
        self.color = color;
    }
    fn set_stroke_color(&mut self, _c: Color) {}
    fn set_line_width(&mut self, _w: f64) {}
    fn set_line_join(&mut self, _j: agg_rust::math_stroke::LineJoin) {}
    fn set_line_cap(&mut self, _c: agg_rust::math_stroke::LineCap) {}
    fn set_miter_limit(&mut self, _l: f64) {}
    fn set_line_dash(&mut self, _d: &[f64], _o: f64) {}
    fn set_blend_mode(&mut self, _m: agg_rust::comp_op::CompOp) {}
    fn set_global_alpha(&mut self, _a: f64) {}
    fn set_fill_rule(&mut self, rule: FillRule) {
        self.rule = rule;
    }
    fn set_font(&mut self, _f: std::sync::Arc<crate::text::Font>) {}
    fn set_font_size(&mut self, _s: f64) {}
    fn clip_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn reset_clip(&mut self) {}
    fn clear(&mut self, _c: Color) {}
    fn begin_path(&mut self) {}
    fn move_to(&mut self, x: f64, y: f64) {
        self.points.push([x, y]);
    }
    fn line_to(&mut self, x: f64, y: f64) {
        self.points.push([x, y]);
    }
    fn cubic_to(&mut self, _a: f64, _b: f64, _c: f64, _d: f64, _e: f64, _f: f64) {}
    fn quad_to(&mut self, _a: f64, _b: f64, _c: f64, _d: f64) {}
    fn arc_to(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64, _ccw: bool) {}
    fn circle(&mut self, _cx: f64, _cy: f64, _r: f64) {}
    fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn rounded_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64, _r: f64) {}
    fn close_path(&mut self) {}
    fn fill(&mut self) {
        self.fills.push(self.color);
        self.rules.push(self.rule);
    }
    fn stroke(&mut self) {}
    fn fill_and_stroke(&mut self) {}
    fn draw_triangles_aa(&mut self, _v: &[[f32; 3]], _i: &[u32], _c: Color) {}
    fn fill_text(&mut self, _t: &str, _x: f64, _y: f64) {}
    fn fill_text_gsv(&mut self, _t: &str, _x: f64, _y: f64, _s: f64) {}
    fn measure_text(&self, _t: &str) -> Option<crate::text::TextMetrics> {
        None
    }
    fn transform(&self) -> agg_rust::trans_affine::TransAffine {
        agg_rust::trans_affine::TransAffine::new()
    }
    fn set_transform(&mut self, _m: agg_rust::trans_affine::TransAffine) {}
    fn reset_transform(&mut self) {}
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _x: f64, _y: f64) {}
    fn rotate(&mut self, _r: f64) {}
    fn scale(&mut self, _x: f64, _y: f64) {}
}

/// Icon space is Y-down (SVG) and agg-gui is Y-up, so the *top* of the
/// artwork must come out at the *top* of the destination rect.
#[test]
fn painting_flips_y_and_fits_the_destination_rect() {
    // A bar across the top quarter of the box.
    let icon = VectorIcon::new(64.0)
        .with_svg_path_nonzero("M0 0 H64 V16 H0 Z", IconColor::Ink)
        .expect("parses");
    let mut rec = PointRecorder::default();
    icon.paint(
        &mut rec,
        Rect::new(100.0, 200.0, 16.0, 16.0),
        Color::black(),
    );

    let ys: Vec<f64> = rec.points.iter().map(|p| p[1]).collect();
    let xs: Vec<f64> = rec.points.iter().map(|p| p[0]).collect();
    let max_y = ys.iter().cloned().fold(f64::MIN, f64::max);
    let min_y = ys.iter().cloned().fold(f64::MAX, f64::min);
    let min_x = xs.iter().cloned().fold(f64::MAX, f64::min);
    let max_x = xs.iter().cloned().fold(f64::MIN, f64::max);
    assert!((max_y - 216.0).abs() < 1e-9, "top of art at {max_y}");
    assert!((min_y - 212.0).abs() < 1e-9, "bottom of the bar at {min_y}");
    assert!((min_x - 100.0).abs() < 1e-9);
    assert!((max_x - 116.0).abs() < 1e-9);
}

/// The ink role follows the caller; a literal does not. And an even-odd
/// path must not leave the shared context in even-odd mode.
#[test]
fn paint_resolves_colour_roles_and_restores_the_fill_rule() {
    let red = Color::from_rgb8(0xF2, 0x0D, 0x0D);
    let icon = VectorIcon::new(64.0)
        .with_svg_path("M0 0 H10 V10 H0 Z", IconColor::Ink, FillRule::EvenOdd)
        .expect("parses")
        .with_svg_path_nonzero("M20 20 H30 V30 H20 Z", IconColor::Literal(red))
        .expect("parses");
    let ink = Color::rgb(0.05, 0.6, 0.9);
    let mut rec = PointRecorder::default();
    icon.paint(&mut rec, Rect::new(0.0, 0.0, 16.0, 16.0), ink);

    assert_eq!(rec.fills, vec![ink, red]);
    assert_eq!(rec.rules, vec![FillRule::EvenOdd, FillRule::NonZero]);
    assert_eq!(rec.rule, FillRule::NonZero, "fill rule left dirty");
}

#[test]
fn an_empty_icon_paints_nothing() {
    let mut rec = PointRecorder::default();
    VectorIcon::new(64.0).paint(&mut rec, Rect::new(0.0, 0.0, 16.0, 16.0), Color::black());
    assert!(rec.points.is_empty());
    assert!(rec.fills.is_empty());
}

#[test]
fn bounds_and_point_count_describe_the_art() {
    let icon = VectorIcon::new(64.0)
        .with_svg_path_nonzero("M4 8 H20 V24 H4 Z", IconColor::Ink)
        .expect("parses");
    assert_eq!(icon.bounds(), Some([4.0, 8.0, 20.0, 24.0]));
    assert!(icon.point_count() >= 4);
}
