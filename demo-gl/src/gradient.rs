//! Hardware linear-gradient fills for `GlGfxCtx`.
//!
//! The SVG walker expresses gradients as bridge-level paint.  This backend
//! renders that paint natively: tess2 still supplies the filled AA geometry,
//! while the fragment shader maps each pixel into gradient space and samples a
//! generated 1-D stop ramp.

use super::*;
use agg_gui::draw_ctx::GradientSpread;

const RAMP_W: usize = 256;

pub(crate) struct GradientPipeline {
    prog: glow::Program,
    res_loc: Option<glow::UniformLocation>,
    ramp_loc: Option<glow::UniformLocation>,
    line_loc: Option<glow::UniformLocation>,
    screen_inv_a_loc: Option<glow::UniformLocation>,
    screen_inv_b_loc: Option<glow::UniformLocation>,
    gradient_inv_a_loc: Option<glow::UniformLocation>,
    gradient_inv_b_loc: Option<glow::UniformLocation>,
    spread_loc: Option<glow::UniformLocation>,
    global_alpha_loc: Option<glow::UniformLocation>,
}

impl GradientPipeline {
    pub(crate) unsafe fn new(gl: &glow::Context) -> Self {
        let prog = crate::gl_support::compile_program(
            gl,
            crate::shaders::GRADIENT_VERT,
            crate::shaders::GRADIENT_FRAG,
        )
        .expect("gradient shader compile/link");
        Self {
            prog,
            res_loc: gl.get_uniform_location(prog, "u_resolution"),
            ramp_loc: gl.get_uniform_location(prog, "u_ramp"),
            line_loc: gl.get_uniform_location(prog, "u_line"),
            screen_inv_a_loc: gl.get_uniform_location(prog, "u_screen_inv_a"),
            screen_inv_b_loc: gl.get_uniform_location(prog, "u_screen_inv_b"),
            gradient_inv_a_loc: gl.get_uniform_location(prog, "u_gradient_inv_a"),
            gradient_inv_b_loc: gl.get_uniform_location(prog, "u_gradient_inv_b"),
            spread_loc: gl.get_uniform_location(prog, "u_spread"),
            global_alpha_loc: gl.get_uniform_location(prog, "u_global_alpha"),
        }
    }
}

impl GlGfxCtx {
    pub(crate) unsafe fn submit_linear_gradient_triangles(
        &self,
        verts: &[[f32; 3]],
        indices: &[u32],
        gradient: &LinearGradientPaint,
        screen_from_local: &TransAffine,
    ) {
        if verts.is_empty() || indices.is_empty() || gradient.stops.is_empty() {
            return;
        }

        let gl = &*self.gl;
        let ramp = gradient_ramp(gradient);
        let tex = gl.create_texture().expect("create gradient ramp texture");
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(tex));
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA as i32,
            RAMP_W as i32,
            1,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            Some(&ramp),
        );

        let mut screen_to_local = *screen_from_local;
        screen_to_local.invert();
        let mut gradient_inverse = gradient.transform;
        gradient_inverse.invert();

        let pipeline = &self.gradient;
        gl.use_program(Some(pipeline.prog));
        gl.uniform_2_f32(pipeline.res_loc.as_ref(), self.viewport.0, self.viewport.1);
        gl.uniform_1_i32(pipeline.ramp_loc.as_ref(), 0);
        gl.uniform_4_f32(
            pipeline.line_loc.as_ref(),
            gradient.x1 as f32,
            gradient.y1 as f32,
            gradient.x2 as f32,
            gradient.y2 as f32,
        );
        set_affine_uniforms(
            gl,
            &screen_to_local,
            pipeline.screen_inv_a_loc.as_ref(),
            pipeline.screen_inv_b_loc.as_ref(),
        );
        set_affine_uniforms(
            gl,
            &gradient_inverse,
            pipeline.gradient_inv_a_loc.as_ref(),
            pipeline.gradient_inv_b_loc.as_ref(),
        );
        gl.uniform_1_i32(
            pipeline.spread_loc.as_ref(),
            match gradient.spread {
                GradientSpread::Pad => 0,
                GradientSpread::Reflect => 1,
                GradientSpread::Repeat => 2,
            },
        );
        gl.uniform_1_f32(pipeline.global_alpha_loc.as_ref(), self.global_alpha as f32);

        gl.bind_vertex_array(Some(self.aa_vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.aa_vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(verts),
            glow::STREAM_DRAW,
        );
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.aa_ibo));
        gl.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            bytemuck::cast_slice(indices),
            glow::STREAM_DRAW,
        );
        gl.draw_elements(glow::TRIANGLES, indices.len() as i32, glow::UNSIGNED_INT, 0);
        gl.bind_vertex_array(None);
        gl.delete_texture(tex);
    }
}

fn set_affine_uniforms(
    gl: &glow::Context,
    m: &TransAffine,
    a_loc: Option<&glow::UniformLocation>,
    b_loc: Option<&glow::UniformLocation>,
) {
    unsafe {
        gl.uniform_4_f32(a_loc, m.sx as f32, m.shy as f32, m.shx as f32, m.sy as f32);
        gl.uniform_2_f32(b_loc, m.tx as f32, m.ty as f32);
    }
}

fn gradient_ramp(gradient: &LinearGradientPaint) -> Vec<u8> {
    let mut ramp = vec![0u8; RAMP_W * 4];
    for x in 0..RAMP_W {
        let t = x as f64 / (RAMP_W - 1) as f64;
        let color = sample_stops(&gradient.stops, t);
        let i = x * 4;
        ramp[i] = (color.r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        ramp[i + 1] = (color.g.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        ramp[i + 2] = (color.b.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        ramp[i + 3] = (color.a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
    ramp
}

fn sample_stops(stops: &[agg_gui::draw_ctx::GradientStop], t: f64) -> Color {
    if t <= stops[0].offset {
        return stops[0].color;
    }
    for pair in stops.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if t <= b.offset {
            let span = (b.offset - a.offset).max(f64::EPSILON);
            let u = ((t - a.offset) / span).clamp(0.0, 1.0) as f32;
            return Color::rgba(
                a.color.r + (b.color.r - a.color.r) * u,
                a.color.g + (b.color.g - a.color.g) * u,
                a.color.b + (b.color.b - a.color.b) * u,
                a.color.a + (b.color.a - a.color.a) * u,
            );
        }
    }
    stops[stops.len() - 1].color
}
