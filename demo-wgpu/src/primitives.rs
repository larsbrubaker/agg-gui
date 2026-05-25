//! Path tessellation methods on [`WgpuGfxCtx`].
//!
//! Mirrors `demo-gl/src/ctx_core/primitives.rs`: converts the accumulated AGG
//! path into AA-tessellated triangle meshes, then pushes the appropriate
//! `DrawCommand` variant (`AaSolid` or `Gradient`) onto `self.commands`.
//!
//! `do_fill` and `do_stroke` are called by `fill()`, `stroke()`, and
//! `fill_and_stroke()` in `draw_ctx_impl.rs`.

use super::*;

use agg_gui::draw_ctx::FillRule;
use agg_gui::gl_renderer::{tessellate_path_aa, tessellate_path_aa_texture};
use agg_rust::conv_curve::ConvCurve;
use agg_rust::conv_dash::ConvDash;
use agg_rust::conv_stroke::ConvStroke;
use agg_rust::conv_transform::ConvTransform;

use crate::gradient::{
    build_linear_gradient_uniforms, build_radial_gradient_uniforms, gradient_ramp,
};

impl WgpuGfxCtx {
    /// Tessellate the current path as a fill and push the correct DrawCommand.
    ///
    /// **Solid fills** take the agg-sharp-port texture-AA path —
    /// `tessellate_path_aa_texture` emits a triangle-fan mesh whose
    /// texcoords drive the 1024-wide alpha-step texture, producing a
    /// sub-texel-wide AA transition right on the polygon edge (no
    /// outward halo bleeding into neighbours).
    ///
    /// **Gradient fills** still take the legacy per-vertex-α halo path
    /// for now — the gradient pipeline doesn't yet read the alpha-step
    /// texture; migrating it is a follow-up.
    pub(crate) fn do_fill(&mut self) {
        let transform = *self.ctm();
        let fill_rule = self.fill_rule;
        let has_gradient =
            self.fill_linear_gradient.is_some() || self.fill_radial_gradient.is_some();

        if has_gradient {
            let tess = {
                let mut curves = ConvCurve::new(&mut self.path);
                let mut transformed = ConvTransform::new(&mut curves, transform);
                tessellate_path_aa(&mut transformed, 1.0, fill_rule)
            };
            if let Some((verts, idx)) = tess {
                self.push_fill_tess(verts, idx, &transform);
            }
            return;
        }

        let tess = {
            let mut curves = ConvCurve::new(&mut self.path);
            let mut transformed = ConvTransform::new(&mut curves, transform);
            tessellate_path_aa_texture(&mut transformed, fill_rule)
        };
        if let Some((verts, indices)) = tess {
            self.commands.push(DrawCommand::AaTexture {
                verts,
                indices,
                color: self.fill_color,
                global_alpha: self.global_alpha as f32,
                clip: self.current_clip(),
            });
        }
    }

    /// Tessellate the current path as a stroke and push the correct DrawCommand.
    ///
    /// Same split as `do_fill`: solid strokes go through the agg-sharp
    /// texture-AA path; gradient strokes keep the legacy halo path.
    pub(crate) fn do_stroke(&mut self) {
        let transform = *self.ctm();
        let width = self.line_width;
        let join = self.line_join;
        let cap = self.line_cap;
        let miter_limit = self.miter_limit;
        let dashes = self.line_dash.clone();
        let dash_offset = self.dash_offset;
        let has_gradient =
            self.stroke_linear_gradient.is_some() || self.stroke_radial_gradient.is_some();

        if has_gradient {
            let tess = {
                let mut curves = ConvCurve::new(&mut self.path);
                if dashes.is_empty() {
                    let mut stroke = ConvStroke::new(&mut curves);
                    stroke.set_width(width);
                    stroke.set_line_join(join);
                    stroke.set_line_cap(cap);
                    stroke.set_miter_limit(miter_limit);
                    let mut transformed = ConvTransform::new(&mut stroke, transform);
                    tessellate_path_aa(&mut transformed, 1.0, FillRule::NonZero)
                } else {
                    let mut dash = ConvDash::new(&mut curves);
                    configure_dashes(&mut dash, &dashes, dash_offset);
                    let mut stroke = ConvStroke::new(dash);
                    stroke.set_width(width);
                    stroke.set_line_join(join);
                    stroke.set_line_cap(cap);
                    stroke.set_miter_limit(miter_limit);
                    let mut transformed = ConvTransform::new(&mut stroke, transform);
                    tessellate_path_aa(&mut transformed, 1.0, FillRule::NonZero)
                }
            };
            if let Some((verts, idx)) = tess {
                self.push_stroke_tess(verts, idx, &transform);
            }
            return;
        }

        let tess = {
            let mut curves = ConvCurve::new(&mut self.path);
            if dashes.is_empty() {
                let mut stroke = ConvStroke::new(&mut curves);
                stroke.set_width(width);
                stroke.set_line_join(join);
                stroke.set_line_cap(cap);
                stroke.set_miter_limit(miter_limit);
                let mut transformed = ConvTransform::new(&mut stroke, transform);
                tessellate_path_aa_texture(&mut transformed, FillRule::NonZero)
            } else {
                let mut dash = ConvDash::new(&mut curves);
                configure_dashes(&mut dash, &dashes, dash_offset);
                let mut stroke = ConvStroke::new(dash);
                stroke.set_width(width);
                stroke.set_line_join(join);
                stroke.set_line_cap(cap);
                stroke.set_miter_limit(miter_limit);
                let mut transformed = ConvTransform::new(&mut stroke, transform);
                tessellate_path_aa_texture(&mut transformed, FillRule::NonZero)
            }
        };

        if let Some((verts, indices)) = tess {
            self.commands.push(DrawCommand::AaTexture {
                verts,
                indices,
                color: self.stroke_color,
                global_alpha: self.global_alpha as f32,
                clip: self.current_clip(),
            });
        }
    }

    /// Dispatch tessellated fill triangles to the correct DrawCommand variant.
    fn push_fill_tess(&mut self, verts: Vec<[f32; 3]>, idx: Vec<u32>, transform: &TransAffine) {
        if let Some(gradient) = self.fill_linear_gradient.clone() {
            let resolution = [self.viewport.0, self.viewport.1];
            let uniforms = build_linear_gradient_uniforms(
                &gradient,
                transform,
                resolution,
                self.global_alpha as f32,
            );
            let ramp = gradient_ramp(&gradient.stops);
            self.commands.push(DrawCommand::Gradient {
                verts,
                indices: idx,
                uniforms,
                ramp,
                clip: self.current_clip(),
            });
        } else if let Some(gradient) = self.fill_radial_gradient.clone() {
            let resolution = [self.viewport.0, self.viewport.1];
            let uniforms = build_radial_gradient_uniforms(
                &gradient,
                transform,
                resolution,
                self.global_alpha as f32,
            );
            let ramp = gradient_ramp(&gradient.stops);
            self.commands.push(DrawCommand::Gradient {
                verts,
                indices: idx,
                uniforms,
                ramp,
                clip: self.current_clip(),
            });
        } else {
            self.commands.push(DrawCommand::AaSolid {
                verts,
                indices: idx,
                color: self.fill_color,
                global_alpha: self.global_alpha as f32,
                clip: self.current_clip(),
            });
        }
    }

    /// Dispatch tessellated stroke triangles to the correct DrawCommand variant.
    fn push_stroke_tess(&mut self, verts: Vec<[f32; 3]>, idx: Vec<u32>, transform: &TransAffine) {
        if let Some(gradient) = self.stroke_linear_gradient.clone() {
            let resolution = [self.viewport.0, self.viewport.1];
            let uniforms = build_linear_gradient_uniforms(
                &gradient,
                transform,
                resolution,
                self.global_alpha as f32,
            );
            let ramp = gradient_ramp(&gradient.stops);
            self.commands.push(DrawCommand::Gradient {
                verts,
                indices: idx,
                uniforms,
                ramp,
                clip: self.current_clip(),
            });
        } else if let Some(gradient) = self.stroke_radial_gradient.clone() {
            let resolution = [self.viewport.0, self.viewport.1];
            let uniforms = build_radial_gradient_uniforms(
                &gradient,
                transform,
                resolution,
                self.global_alpha as f32,
            );
            let ramp = gradient_ramp(&gradient.stops);
            self.commands.push(DrawCommand::Gradient {
                verts,
                indices: idx,
                uniforms,
                ramp,
                clip: self.current_clip(),
            });
        } else {
            self.commands.push(DrawCommand::AaSolid {
                verts,
                indices: idx,
                color: self.stroke_color,
                global_alpha: self.global_alpha as f32,
                clip: self.current_clip(),
            });
        }
    }
}

fn configure_dashes<VS: agg_rust::basics::VertexSource>(
    dash: &mut ConvDash<VS>,
    dashes: &[f64],
    dash_offset: f64,
) {
    let mut chunks = dashes.chunks_exact(2);
    for pair in &mut chunks {
        dash.add_dash(pair[0], pair[1]);
    }
    if let Some(&last) = chunks.remainder().first() {
        dash.add_dash(last, last);
    }
    dash.dash_start(dash_offset);
}
