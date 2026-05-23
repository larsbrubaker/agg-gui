//! Pure-function tests for `ColorWheelPicker`'s helpers.
//!
//! These cover the HSV / hex / barycentric math that the widget code
//! relies on; widget-level App integration tests live in
//! `agg-gui/src/tests/widgets.rs`.

use super::hsv_math::*;
use crate::color::Color;

/// HSV → RGB → HSV should round-trip every primary / secondary / neutral
/// colour with no drift bigger than 1 / 1024.  Catches sign / scale bugs
/// in the wheel and triangle conversions.
#[test]
fn hsv_round_trip_primaries() {
    for (h, s, v) in [
        (0.0, 1.0, 1.0),     // red
        (60.0, 1.0, 1.0),    // yellow
        (120.0, 1.0, 1.0),   // green
        (180.0, 1.0, 1.0),   // cyan
        (240.0, 1.0, 1.0),   // blue
        (300.0, 1.0, 1.0),   // magenta
        (35.0, 0.7, 0.85),   // arbitrary orange
        (210.0, 0.42, 0.62), // arbitrary cool grey-blue
    ] {
        let (r, g, b) = hsv_to_rgb(h, s, v);
        let (h2, s2, v2) = rgb_to_hsv(r, g, b);
        assert!(
            (h2 - h).abs() < 1.0 / 1024.0,
            "hue drift: {h}→{h2} (input s={s} v={v})"
        );
        assert!(
            (s2 - s).abs() < 1.0 / 1024.0,
            "sat drift: {s}→{s2} (input h={h} v={v})"
        );
        assert!(
            (v2 - v).abs() < 1.0 / 1024.0,
            "val drift: {v}→{v2} (input h={h} s={s})"
        );
    }
}

#[test]
fn hsv_grays_have_zero_saturation() {
    for t in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
        let (_, s, v) = rgb_to_hsv(t, t, t);
        assert_eq!(s, 0.0);
        assert!((v - t).abs() < 1e-6);
    }
}

#[test]
fn format_hex_omits_alpha_when_opaque() {
    assert_eq!(format_hex(Color::rgb(1.0, 0.0, 0.0)), "#FF0000");
    assert_eq!(format_hex(Color::rgb(0.0, 0.5, 1.0)), "#0080FF");
}

#[test]
fn format_hex_includes_alpha_when_translucent() {
    assert_eq!(format_hex(Color::rgba(1.0, 1.0, 1.0, 0.5)), "#FFFFFF80");
    assert_eq!(format_hex(Color::transparent()), "#00000000");
}

#[test]
fn parse_hex_accepts_short_and_long_forms() {
    let red = parse_hex("#F00").unwrap();
    assert!((red.r - 1.0).abs() < 1e-6 && red.g == 0.0 && red.b == 0.0 && red.a == 1.0);

    let translucent = parse_hex("#F00A").unwrap();
    assert!((translucent.a - (0xAA as f32 / 255.0)).abs() < 1e-6);

    let lime = parse_hex("80FF00").unwrap();
    assert!((lime.r - (0x80 as f32 / 255.0)).abs() < 1e-6);

    let exact = parse_hex("#12345678").unwrap();
    assert_eq!(format_hex(exact), "#12345678");
}

#[test]
fn parse_hex_rejects_garbage() {
    assert!(parse_hex("not-a-colour").is_none());
    assert!(parse_hex("#GG0000").is_none());
    assert!(parse_hex("#12345").is_none()); // wrong length
}

#[test]
fn barycentric_centroid_has_equal_weights() {
    let a = (0.0, 0.0);
    let b = (1.0, 0.0);
    let c = (0.5, 1.0);
    let centroid = ((a.0 + b.0 + c.0) / 3.0, (a.1 + b.1 + c.1) / 3.0);
    let (wa, wb, wc) = barycentric(centroid, a, b, c);
    assert!((wa - 1.0 / 3.0).abs() < 1e-9);
    assert!((wb - 1.0 / 3.0).abs() < 1e-9);
    assert!((wc - 1.0 / 3.0).abs() < 1e-9);
}

#[test]
fn barycentric_outside_triangle_has_negative_weight() {
    let a = (0.0, 0.0);
    let b = (1.0, 0.0);
    let c = (0.5, 1.0);
    let outside = (2.0, 0.0); // far right of vertex b
    let (wa, _wb, wc) = barycentric(outside, a, b, c);
    assert!(
        wa < 0.0 || wc < 0.0,
        "expected one weight < 0 (got wa={wa} wc={wc})"
    );
}

#[test]
fn sv_round_trip_through_triangle() {
    // Standard inscribed triangle, hue=0 → vertex 1 to the right.
    let (v1, v2, v3) = sv_triangle_vertices(100.0, 100.0, 50.0, 0.0);
    for (s, v) in [
        (1.0_f32, 1.0_f32),
        (0.0, 1.0),
        (0.0, 0.0),
        (0.5, 0.5),
        (0.25, 0.8),
        (0.75, 0.3),
    ] {
        let p = sv_to_point(s, v, v1, v2, v3);
        let (w1, w2, w3) = barycentric(p, v1, v2, v3);
        let recovered_val = (w1 + w2) as f32;
        let recovered_sat = if recovered_val > 0.0 {
            (w1 / (w1 + w2)) as f32
        } else {
            0.0
        };
        assert!((recovered_val - v).abs() < 1e-6, "val drift {v} -> {recovered_val}");
        if v > 1e-6 {
            assert!((recovered_sat - s).abs() < 1e-6, "sat drift {s} -> {recovered_sat}");
        }
        let weight_sum = w1 + w2 + w3;
        assert!((weight_sum - 1.0).abs() < 1e-6);
    }
}

#[test]
fn sv_triangle_vertex_one_lies_on_the_ring_at_the_hue_angle() {
    // hue=90° → vertex 1 should be straight up from the centre (Y-up).
    let cx = 50.0;
    let cy = 50.0;
    let r = 30.0;
    let (v1, _, _) = sv_triangle_vertices(cx, cy, r, 90.0);
    assert!((v1.0 - cx).abs() < 1e-6);
    assert!((v1.1 - (cy + r)).abs() < 1e-6);
}
