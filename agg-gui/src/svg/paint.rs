//! SVG paint-server mapping for the `DrawCtx` bridge.
//!
//! This keeps solid colors and gradient paint setup separate from the tree
//! walker so adding future pattern support does not grow `svg.rs`.

use crate::color::Color;
use crate::draw_ctx::{
    DrawCtx, GradientSpread, GradientStop, LinearGradientPaint, RadialGradientPaint,
};

pub(super) fn apply_fill_paint(ctx: &mut dyn DrawCtx, paint: &usvg::Paint, opacity: f32) -> bool {
    match paint {
        usvg::Paint::Color(_) => {
            if let Some(color) = solid_paint(paint, opacity) {
                ctx.set_fill_color(color);
                true
            } else {
                false
            }
        }
        usvg::Paint::LinearGradient(gradient) => {
            if !ctx.supports_fill_linear_gradient() {
                return false;
            }
            let stops = gradient_stops(gradient.stops(), opacity);
            if stops.is_empty() {
                return false;
            }
            ctx.set_fill_linear_gradient(LinearGradientPaint {
                x1: gradient.x1() as f64,
                y1: gradient.y1() as f64,
                x2: gradient.x2() as f64,
                y2: gradient.y2() as f64,
                transform: super::to_trans_affine(gradient.transform()),
                spread: map_spread(gradient.spread_method()),
                stops,
            });
            true
        }
        usvg::Paint::RadialGradient(gradient) => {
            if !ctx.supports_fill_radial_gradient() {
                return false;
            }
            let stops = gradient_stops(gradient.stops(), opacity);
            if stops.is_empty() {
                return false;
            }
            ctx.set_fill_radial_gradient(RadialGradientPaint {
                cx: gradient.cx() as f64,
                cy: gradient.cy() as f64,
                r: gradient.r().get() as f64,
                fx: gradient.fx() as f64,
                fy: gradient.fy() as f64,
                transform: super::to_trans_affine(gradient.transform()),
                spread: map_spread(gradient.spread_method()),
                stops,
            });
            true
        }
        usvg::Paint::Pattern(_) => false,
    }
}

pub(super) fn apply_stroke_paint(ctx: &mut dyn DrawCtx, paint: &usvg::Paint, opacity: f32) -> bool {
    match paint {
        usvg::Paint::Color(_) => {
            if let Some(color) = solid_paint(paint, opacity) {
                ctx.set_stroke_color(color);
                true
            } else {
                false
            }
        }
        usvg::Paint::LinearGradient(gradient) => {
            if !ctx.supports_stroke_linear_gradient() {
                return false;
            }
            let stops = gradient_stops(gradient.stops(), opacity);
            if stops.is_empty() {
                return false;
            }
            ctx.set_stroke_linear_gradient(LinearGradientPaint {
                x1: gradient.x1() as f64,
                y1: gradient.y1() as f64,
                x2: gradient.x2() as f64,
                y2: gradient.y2() as f64,
                transform: super::to_trans_affine(gradient.transform()),
                spread: map_spread(gradient.spread_method()),
                stops,
            });
            true
        }
        usvg::Paint::RadialGradient(gradient) => {
            if !ctx.supports_stroke_radial_gradient() {
                return false;
            }
            let stops = gradient_stops(gradient.stops(), opacity);
            if stops.is_empty() {
                return false;
            }
            ctx.set_stroke_radial_gradient(RadialGradientPaint {
                cx: gradient.cx() as f64,
                cy: gradient.cy() as f64,
                r: gradient.r().get() as f64,
                fx: gradient.fx() as f64,
                fy: gradient.fy() as f64,
                transform: super::to_trans_affine(gradient.transform()),
                spread: map_spread(gradient.spread_method()),
                stops,
            });
            true
        }
        usvg::Paint::Pattern(_) => false,
    }
}

fn solid_paint(paint: &usvg::Paint, opacity: f32) -> Option<Color> {
    match paint {
        usvg::Paint::Color(color) => Some(Color::rgba(
            color.red as f32 / 255.0,
            color.green as f32 / 255.0,
            color.blue as f32 / 255.0,
            opacity,
        )),
        usvg::Paint::LinearGradient(_)
        | usvg::Paint::RadialGradient(_)
        | usvg::Paint::Pattern(_) => None,
    }
}

fn gradient_stops(stops: &[usvg::Stop], opacity: f32) -> Vec<GradientStop> {
    stops
        .iter()
        .map(|stop| {
            let color = stop.color();
            GradientStop {
                offset: stop.offset().get() as f64,
                color: Color::rgba(
                    color.red as f32 / 255.0,
                    color.green as f32 / 255.0,
                    color.blue as f32 / 255.0,
                    opacity * stop.opacity().get(),
                ),
            }
        })
        .collect()
}

fn map_spread(spread: usvg::SpreadMethod) -> GradientSpread {
    match spread {
        usvg::SpreadMethod::Pad => GradientSpread::Pad,
        usvg::SpreadMethod::Reflect => GradientSpread::Reflect,
        usvg::SpreadMethod::Repeat => GradientSpread::Repeat,
    }
}
