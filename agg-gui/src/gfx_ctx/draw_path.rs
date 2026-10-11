//! `GfxCtx::fill`, `stroke` and `fill_and_stroke`: draw the accumulated path
//! with the current paint (solid colour, gradient or pattern), blend mode,
//! clip, transform and anti-aliasing. The rasterization itself is in
//! `draw_impl.rs` (solid fills and strokes) and `sampled.rs` (paint servers);
//! path construction stays in `gfx_ctx.rs`.

use super::*;

impl GfxCtx<'_> {
    // -------------------------------------------------------------------------
    // Drawing
    // -------------------------------------------------------------------------

    /// Fill the accumulated path.
    pub fn fill(&mut self) {
        let mode = self.state.blend_mode;
        let clip = self.state.clip;
        let fill_rule = self.state.fill_rule;
        let transform = self.state.transform;
        let anti_alias = self.state.anti_alias;
        let fb = active_fb(self.base_fb, &mut self.layer_stack);
        if let Some(gradient) = self.state.fill_linear_gradient.clone() {
            sampled::rasterize_linear_gradient_fill(
                fb,
                &mut self.path,
                &gradient,
                self.state.global_alpha as f32,
                mode,
                clip,
                fill_rule,
                &transform,
                anti_alias,
            );
        } else if let Some(gradient) = self.state.fill_radial_gradient.clone() {
            sampled::rasterize_radial_gradient_fill(
                fb,
                &mut self.path,
                &gradient,
                self.state.global_alpha as f32,
                mode,
                clip,
                fill_rule,
                &transform,
                anti_alias,
            );
        } else if let Some(pattern) = self.state.fill_pattern.clone() {
            sampled::rasterize_pattern_fill(
                fb,
                &mut self.path,
                &pattern,
                self.state.global_alpha as f32,
                mode,
                clip,
                fill_rule,
                &transform,
                anti_alias,
            );
        } else {
            let mut color = self.state.fill_color;
            color.a *= self.state.global_alpha as f32;
            let rgba = color.to_rgba8();
            rasterize_fill(
                fb,
                &mut self.path,
                &rgba,
                mode,
                clip,
                fill_rule,
                &transform,
                anti_alias,
            );
        }
    }

    /// Stroke the accumulated path.
    pub fn stroke(&mut self) {
        let mut color = self.state.stroke_color;
        color.a *= self.state.global_alpha as f32;
        let rgba = color.to_rgba8();
        let width = self.state.line_width;
        let join = self.state.line_join;
        let cap = self.state.line_cap;
        let miter_limit = self.state.miter_limit;
        let dashes = self.state.line_dash.clone();
        let dash_offset = self.state.dash_offset;
        let mode = self.state.blend_mode;
        let clip = self.state.clip;
        let transform = self.state.transform;
        let anti_alias = self.state.anti_alias;
        let fb = active_fb(self.base_fb, &mut self.layer_stack);
        if let Some(gradient) = self.state.stroke_linear_gradient.clone() {
            let mut outline = stroke::materialize_stroke_outline(&mut self.path, &self.state);
            sampled::rasterize_linear_gradient_fill(
                fb,
                &mut outline,
                &gradient,
                self.state.global_alpha as f32,
                mode,
                clip,
                FillRule::NonZero,
                &transform,
                anti_alias,
            );
        } else if let Some(gradient) = self.state.stroke_radial_gradient.clone() {
            let mut outline = stroke::materialize_stroke_outline(&mut self.path, &self.state);
            sampled::rasterize_radial_gradient_fill(
                fb,
                &mut outline,
                &gradient,
                self.state.global_alpha as f32,
                mode,
                clip,
                FillRule::NonZero,
                &transform,
                anti_alias,
            );
        } else if let Some(pattern) = self.state.stroke_pattern.clone() {
            let mut outline = stroke::materialize_stroke_outline(&mut self.path, &self.state);
            sampled::rasterize_pattern_fill(
                fb,
                &mut outline,
                &pattern,
                self.state.global_alpha as f32,
                mode,
                clip,
                FillRule::NonZero,
                &transform,
                anti_alias,
            );
        } else {
            rasterize_stroke(
                fb,
                &mut self.path,
                &rgba,
                width,
                join,
                cap,
                miter_limit,
                &dashes,
                dash_offset,
                mode,
                clip,
                &transform,
                anti_alias,
            );
        }
    }

    /// Fill then stroke the accumulated path in one call.
    pub fn fill_and_stroke(&mut self) {
        self.fill();
        self.stroke();
    }
}
