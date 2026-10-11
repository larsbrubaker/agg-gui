//! Paint definitions consumed by [`DrawCtx`](crate::draw_ctx::DrawCtx).
//!
//! Solid color fills are expressed directly via [`Color`]; this module hosts
//! the richer paint kinds that need their own data: [`LinearGradientPaint`],
//! [`RadialGradientPaint`], and [`PatternPaint`], plus the supporting
//! [`GradientStop`] / [`GradientSpread`] / [`FillRule`] types and the CPU-side
//! `sample` implementations the software backend uses.
//!
//! `draw_ctx` re-exports every public name here, so existing call sites that
//! reach for `agg_gui::draw_ctx::RadialGradientPaint` (etc.) continue to
//! resolve unchanged.

use std::sync::Arc;

use crate::color::Color;
use agg_rust::trans_affine::TransAffine;

/// Fill rule used when rasterizing closed paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FillRule {
    /// Non-zero winding rule.
    #[default]
    NonZero,
    /// Even-odd parity rule.
    EvenOdd,
}

/// How a gradient behaves outside the normalized `0..=1` range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GradientSpread {
    /// Clamp to the nearest edge stop.
    #[default]
    Pad,
    /// Mirror each repeated interval.
    Reflect,
    /// Repeat the gradient ramp.
    Repeat,
}

/// One color stop in a bridge-level gradient paint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    pub offset: f64,
    pub color: Color,
}

/// Linear gradient fill paint expressed in local drawing coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearGradientPaint {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub transform: TransAffine,
    pub spread: GradientSpread,
    pub stops: Vec<GradientStop>,
}

impl LinearGradientPaint {
    pub fn sample(&self, mut x: f64, mut y: f64) -> Color {
        if self.stops.is_empty() {
            return Color::transparent();
        }

        self.transform.inverse_transform(&mut x, &mut y);

        let dx = self.x2 - self.x1;
        let dy = self.y2 - self.y1;
        let len2 = dx * dx + dy * dy;
        let t = if len2 > f64::EPSILON {
            ((x - self.x1) * dx + (y - self.y1) * dy) / len2
        } else {
            0.0
        };
        let t = apply_spread(t, self.spread);

        sample_stops(&self.stops, t)
    }
}

/// Radial/focal gradient fill paint expressed in local drawing coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct RadialGradientPaint {
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
    pub fx: f64,
    pub fy: f64,
    pub transform: TransAffine,
    pub spread: GradientSpread,
    pub stops: Vec<GradientStop>,
}

impl RadialGradientPaint {
    /// Convenience constructor for the common case: focal point at the centre,
    /// identity transform, `Pad` spread. Stops are `(offset, color)` pairs.
    pub fn centered(cx: f64, cy: f64, r: f64, stops: &[(f64, Color)]) -> Self {
        Self {
            cx,
            cy,
            r,
            fx: cx,
            fy: cy,
            transform: TransAffine::default(),
            spread: GradientSpread::Pad,
            stops: stops
                .iter()
                .map(|(offset, color)| GradientStop {
                    offset: *offset,
                    color: *color,
                })
                .collect(),
        }
    }

    pub fn sample(&self, mut x: f64, mut y: f64) -> Color {
        if self.stops.is_empty() {
            return Color::transparent();
        }

        self.transform.inverse_transform(&mut x, &mut y);

        let dx = x - self.fx;
        let dy = y - self.fy;
        let fx = self.fx - self.cx;
        let fy = self.fy - self.cy;
        if self.r > f64::EPSILON && fx * fx + fy * fy >= self.r * self.r {
            // A focus on or outside the circle: draw the two-point cone, leaving
            // what no circle of it reaches unpainted (see `focal_cone_t`).
            return match focal_cone_t(dx, dy, -fx, -fy, self.r) {
                Some(t) => sample_stops(&self.stops, apply_spread(t, self.spread)),
                None => Color::transparent(),
            };
        }
        let a = dx * dx + dy * dy;
        let t = if a <= f64::EPSILON || self.r <= f64::EPSILON {
            0.0
        } else {
            let b = 2.0 * (fx * dx + fy * dy);
            let c = fx * fx + fy * fy - self.r * self.r;
            let disc = (b * b - 4.0 * a * c).max(0.0);
            let k = (-b + disc.sqrt()) / (2.0 * a);
            if k > f64::EPSILON {
                1.0 / k
            } else {
                0.0
            }
        };
        sample_stops(&self.stops, apply_spread(t, self.spread))
    }
}

/// The gradient position of a point of a radial gradient whose focus lies on
/// or outside its circle, drawn as the two-point cone resvg (and canvas) draw:
/// the circles grow from the focus (radius 0 at t = 0) to the gradient circle
/// (t = 1), and a point takes the largest t whose circle, with a radius not
/// below 0, passes through it. `None` where no circle reaches the point, which
/// stays unpainted rather than the focus being pulled inside the circle. Ports
/// agg-sharp's `SvgFocalCone.Solve` with fr = 0 (usvg 0.45 has no `fr`):
/// q = p - f, e = c - f, solving a t^2 - 2 b t + k = 0 where a = e.e - r^2,
/// b = q.e and k = q.q.
fn focal_cone_t(qx: f64, qy: f64, ex: f64, ey: f64, r: f64) -> Option<f64> {
    let a = ex * ex + ey * ey - r * r;
    let b = qx * ex + qy * ey;
    let k = qx * qx + qy * qy;
    if a.abs() <= 1e-9 * r * r {
        // The focus sits on the circle: one root.
        if k == 0.0 {
            return Some(0.0);
        }
        if b == 0.0 {
            return None;
        }
        let t = k / (2.0 * b);
        return if t * r >= 0.0 { Some(t) } else { None };
    }
    let discriminant = b * b - a * k;
    if discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    let (t1, t2) = ((b + root) / a, (b - root) / a);
    let t = t1.max(t2);
    if t * r >= 0.0 {
        return Some(t);
    }
    let t = t1.min(t2);
    if t * r >= 0.0 {
        Some(t)
    } else {
        None
    }
}

/// Repeating raster pattern paint expressed in SVG/user drawing coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct PatternPaint {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub transform: TransAffine,
    /// Straight-alpha RGBA tile pixels in bottom-up row order.
    pub pixels: Arc<Vec<u8>>,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

impl PatternPaint {
    pub fn sample(&self, mut x: f64, mut y: f64) -> Color {
        if self.width <= f64::EPSILON
            || self.height <= f64::EPSILON
            || self.pixel_width == 0
            || self.pixel_height == 0
            || self.pixels.is_empty()
        {
            return Color::transparent();
        }

        self.transform.inverse_transform(&mut x, &mut y);
        let tx = (x - self.x).rem_euclid(self.width);
        let ty_down = (y - self.y).rem_euclid(self.height);
        let px = ((tx / self.width) * self.pixel_width as f64)
            .floor()
            .clamp(0.0, self.pixel_width.saturating_sub(1) as f64) as usize;
        let py = (((self.height - ty_down) / self.height) * self.pixel_height as f64)
            .floor()
            .clamp(0.0, self.pixel_height.saturating_sub(1) as f64) as usize;
        let i = (py * self.pixel_width as usize + px) * 4;
        if i + 3 >= self.pixels.len() {
            return Color::transparent();
        }

        Color::rgba(
            self.pixels[i] as f32 / 255.0,
            self.pixels[i + 1] as f32 / 255.0,
            self.pixels[i + 2] as f32 / 255.0,
            self.pixels[i + 3] as f32 / 255.0,
        )
    }
}

fn apply_spread(t: f64, spread: GradientSpread) -> f64 {
    match spread {
        GradientSpread::Pad => t.clamp(0.0, 1.0),
        GradientSpread::Repeat => t - t.floor(),
        GradientSpread::Reflect => {
            let period = t.rem_euclid(2.0);
            if period <= 1.0 {
                period
            } else {
                2.0 - period
            }
        }
    }
}

fn sample_stops(stops: &[GradientStop], t: f64) -> Color {
    if t <= stops[0].offset {
        return stops[0].color;
    }
    for pair in stops.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if t <= b.offset {
            let span = (b.offset - a.offset).max(f64::EPSILON);
            let u = ((t - a.offset) / span).clamp(0.0, 1.0) as f32;
            return lerp_color(a.color, b.color, u);
        }
    }
    stops[stops.len() - 1].color
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color::rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}
