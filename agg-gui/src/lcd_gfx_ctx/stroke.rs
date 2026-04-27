//! Stroke rendering for `LcdGfxCtx`.
//!
//! Strokes are materialized as filled outlines so solid and gradient strokes
//! can share the same LCD coverage mask pipeline as ordinary path fills.

use super::*;

pub(super) fn stroke(ctx: &mut LcdGfxCtx<'_>) {
    let mut color = ctx.state.stroke_color;
    color.a *= ctx.state.global_alpha as f32;
    let mut materialized = PathStorage::new();
    {
        let mut curves = ConvCurve::new(&mut ctx.path);
        if ctx.state.line_dash.is_empty() {
            let mut stroke = ConvStroke::new(&mut curves);
            configure_stroke(
                &mut stroke,
                ctx.state.line_width,
                ctx.state.line_join,
                ctx.state.line_cap,
                ctx.state.miter_limit,
            );
            materialized.concat_path(&mut stroke, 0);
        } else {
            let mut dash = ConvDash::new(&mut curves);
            configure_dashes(&mut dash, &ctx.state.line_dash, ctx.state.dash_offset);
            let mut stroke = ConvStroke::new(dash);
            configure_stroke(
                &mut stroke,
                ctx.state.line_width,
                ctx.state.line_join,
                ctx.state.line_cap,
                ctx.state.miter_limit,
            );
            materialized.concat_path(&mut stroke, 0);
        }
    }

    let xform = ctx.state.transform;
    let clip = ctx.state.clip;
    let global_alpha = ctx.state.global_alpha as f32;
    if let Some(gradient) = ctx.state.stroke_linear_gradient.clone() {
        gradient::fill_linear_gradient(
            ctx.active_buffer(),
            &mut materialized,
            &gradient,
            global_alpha,
            &xform,
            clip,
            FillRule::NonZero,
        );
    } else if let Some(gradient) = ctx.state.stroke_radial_gradient.clone() {
        gradient::fill_radial_gradient(
            ctx.active_buffer(),
            &mut materialized,
            &gradient,
            global_alpha,
            &xform,
            clip,
            FillRule::NonZero,
        );
    } else if let Some(pattern) = ctx.state.stroke_pattern.clone() {
        gradient::fill_pattern(
            ctx.active_buffer(),
            &mut materialized,
            &pattern,
            global_alpha,
            &xform,
            clip,
            FillRule::NonZero,
        );
    } else {
        ctx.active_buffer()
            .fill_path(&mut materialized, color, &xform, clip, FillRule::NonZero);
    }
}
