//! Colour-space math used by `ColorWheelPicker` and its child widgets.
//!
//! Mirrors NodeDesigner's `color-picker.js` helpers:
//!
//! - HSV with **hue in degrees 0..360** (not normalised 0..1 like the
//!   sibling `ColorPicker` widget) so the wheel angle maps directly.
//! - Inverse converters round-trip a colour through the picker without
//!   drifting; we test that explicitly in `tests.rs`.
//! - `barycentric_in_triangle` returns the three weights for a point
//!   `(p, a, b, c)`; the wheel-picker uses them both for hit-testing
//!   (all three in `0..=1` ⇒ inside) and for mapping the cursor's
//!   position back to saturation / value via `val = w_a + w_b` and
//!   `sat = w_a / val`.
//! - Hex parse / format accepts `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`
//!   (NodeDesigner's expected shapes) and always emits uppercase
//!   `#RRGGBB` (or `#RRGGBBAA` when alpha < 1).

use crate::color::Color;

/// Convert linear-RGB `(r, g, b)` in `[0, 1]` to HSV with `h ∈ [0, 360)`.
///
/// `s` and `v` are in `[0, 1]`.  Returns `(h, s, v)`.
pub fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let v = max;
    let s = if max <= 0.0 { 0.0 } else { d / max };
    let h_unsigned = if d <= 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    let h = if h_unsigned < 0.0 {
        h_unsigned + 360.0
    } else {
        h_unsigned
    };
    (h, s, v)
}

/// Convert HSV with `h` in degrees back to linear RGB in `[0, 1]`.
pub fn hsv_to_rgb(h_deg: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h = h_deg.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r1, g1, b1) = match h.floor() as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (r1 + m, g1 + m, b1 + m)
}

/// Format `c` as `#RRGGBB` (or `#RRGGBBAA` when `c.a < 1`).
pub fn format_hex(c: Color) -> String {
    let r = (c.r * 255.0).round().clamp(0.0, 255.0) as u32;
    let g = (c.g * 255.0).round().clamp(0.0, 255.0) as u32;
    let b = (c.b * 255.0).round().clamp(0.0, 255.0) as u32;
    let a = (c.a * 255.0).round().clamp(0.0, 255.0) as u32;
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

/// Parse `#RGB`, `#RGBA`, `#RRGGBB`, or `#RRGGBBAA` into a `Color`.
///
/// Leading `#` is optional.  Returns `None` for malformed input so the
/// hex input field can keep showing the previous valid colour while the
/// user is mid-edit instead of snapping to black.
pub fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    let expand = |c: char| -> Option<u8> { u8::from_str_radix(&format!("{c}{c}"), 16).ok() };
    let pair = |a: char, b: char| -> Option<u8> { u8::from_str_radix(&format!("{a}{b}"), 16).ok() };
    let bytes: Vec<char> = s.chars().collect();
    let (r, g, b, a) = match bytes.len() {
        3 => (expand(bytes[0])?, expand(bytes[1])?, expand(bytes[2])?, 255u8),
        4 => (
            expand(bytes[0])?,
            expand(bytes[1])?,
            expand(bytes[2])?,
            expand(bytes[3])?,
        ),
        6 => (
            pair(bytes[0], bytes[1])?,
            pair(bytes[2], bytes[3])?,
            pair(bytes[4], bytes[5])?,
            255u8,
        ),
        8 => (
            pair(bytes[0], bytes[1])?,
            pair(bytes[2], bytes[3])?,
            pair(bytes[4], bytes[5])?,
            pair(bytes[6], bytes[7])?,
        ),
        _ => return None,
    };
    Some(Color::from_rgba8(r, g, b, a))
}

/// Compute the three barycentric weights `(w_a, w_b, w_c)` for point `p`
/// against triangle `(a, b, c)`.
///
/// The point is strictly inside the triangle iff all three weights are in
/// `[0, 1]`.  Returns `(0, 0, 0)` for a degenerate (zero-area) triangle.
pub fn barycentric(p: (f64, f64), a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> (f64, f64, f64) {
    let v0x = b.0 - a.0;
    let v0y = b.1 - a.1;
    let v1x = c.0 - a.0;
    let v1y = c.1 - a.1;
    let v2x = p.0 - a.0;
    let v2y = p.1 - a.1;
    let denom = v0x * v1y - v1x * v0y;
    if denom.abs() < f64::EPSILON {
        return (0.0, 0.0, 0.0);
    }
    let w_b = (v2x * v1y - v1x * v2y) / denom;
    let w_c = (v0x * v2y - v2x * v0y) / denom;
    let w_a = 1.0 - w_b - w_c;
    (w_a, w_b, w_c)
}

/// Geometry of the saturation/value triangle inscribed in the hue wheel
/// at the supplied centre and radius, rotated so vertex 1 (pure hue) sits
/// on the ring at `hue_deg`.
///
/// Vertex ordering matches NodeDesigner:
///
/// - `v1` — pure hue colour (saturation = 1, value = 1)
/// - `v2` — white   (saturation = 0, value = 1)
/// - `v3` — black   (saturation = 0, value = 0)
///
/// Coordinates are returned in **agg-gui's Y-up** local space, with the
/// triangle centred on `(cx, cy)`.  Wheel angles increase counter-clockwise
/// (standard `atan2` convention); `hue_deg = 0` puts vertex 1 to the right.
pub fn sv_triangle_vertices(
    cx: f64,
    cy: f64,
    radius: f64,
    hue_deg: f32,
) -> ((f64, f64), (f64, f64), (f64, f64)) {
    let hue_rad = (hue_deg as f64).to_radians();
    let v = |off: f64| {
        let a = hue_rad + off;
        (cx + radius * a.cos(), cy + radius * a.sin())
    };
    (
        v(0.0),
        v(2.0 * std::f64::consts::PI / 3.0),
        v(4.0 * std::f64::consts::PI / 3.0),
    )
}

/// Convert `(saturation, value)` ∈ `[0, 1]²` back to a Cartesian point
/// inside the triangle `(v1, v2, v3)` returned by `sv_triangle_vertices`.
///
/// Inverse of the `val = w1 + w2`, `sat = w1 / val` mapping used in the
/// SV widget's hit-test: the marker position is reconstructed so the
/// crosshair lands on the right pixel after a hex / round-trip update.
pub fn sv_to_point(
    s: f32,
    v: f32,
    v1: (f64, f64),
    v2: (f64, f64),
    v3: (f64, f64),
) -> (f64, f64) {
    let w1 = (s as f64) * (v as f64);
    let w2 = (1.0 - s as f64) * (v as f64);
    let w3 = 1.0 - w1 - w2;
    (
        w1 * v1.0 + w2 * v2.0 + w3 * v3.0,
        w1 * v1.1 + w2 * v2.1 + w3 * v3.1,
    )
}
