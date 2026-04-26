use super::*;

impl GlGfxCtx {
    /// Submit triangles with the given colour.
    ///
    /// `verts` is a slice of screen-space [f32;2] XY pairs.
    /// `indices` is a list of triangle vertex indices into `verts`.
    pub(crate) unsafe fn draw_triangles(&self, verts: &[[f32; 2]], indices: &[u32], color: Color) {
        if verts.is_empty() || indices.is_empty() {
            return;
        }

        let a = (color.a * self.global_alpha as f32).clamp(0.0, 1.0);

        self.gl.use_program(Some(self.prog));
        if let Some(ref loc) = self.res_loc {
            self.gl
                .uniform_2_f32(Some(loc), self.viewport.0, self.viewport.1);
        }
        if let Some(ref loc) = self.color_loc {
            self.gl
                .uniform_4_f32(Some(loc), color.r, color.g, color.b, a);
        }

        // Bind VAO (restores attribute pointer, VBO association, and IBO binding).
        self.gl.bind_vertex_array(Some(self.vao));

        // Upload vertex data.
        self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
        self.gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(verts),
            glow::STREAM_DRAW,
        );

        // Upload index data to the persistent IBO (already bound in the VAO).
        self.gl
            .bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.ibo));
        self.gl.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            bytemuck::cast_slice(indices),
            glow::STREAM_DRAW,
        );

        self.gl
            .draw_elements(glow::TRIANGLES, indices.len() as i32, glow::UNSIGNED_INT, 0);

        self.gl.bind_vertex_array(None);
    }

    /// Submit AA triangles — vertices are `[x, y, alpha]` triples.
    ///
    /// Interior triangles and halo-strip triangles are blended uniformly via
    /// the AA solid shader: per-vertex `a_alpha` is interpolated across the
    /// primitive, so halo quads with inner=1.0 / outer=0.0 produce an
    /// analytic edge-coverage ramp one pixel wide.
    pub(crate) unsafe fn submit_aa_triangles(
        &self,
        verts: &[[f32; 3]],
        indices: &[u32],
        color: Color,
    ) {
        if verts.is_empty() || indices.is_empty() {
            return;
        }
        let a = (color.a * self.global_alpha as f32).clamp(0.0, 1.0);

        self.gl.use_program(Some(self.aa_prog));
        if let Some(ref loc) = self.aa_res_loc {
            self.gl
                .uniform_2_f32(Some(loc), self.viewport.0, self.viewport.1);
        }
        if let Some(ref loc) = self.aa_color_loc {
            self.gl
                .uniform_4_f32(Some(loc), color.r, color.g, color.b, a);
        }

        self.gl.bind_vertex_array(Some(self.aa_vao));

        self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.aa_vbo));
        self.gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(verts),
            glow::STREAM_DRAW,
        );

        self.gl
            .bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.aa_ibo));
        self.gl.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            bytemuck::cast_slice(indices),
            glow::STREAM_DRAW,
        );

        self.gl
            .draw_elements(glow::TRIANGLES, indices.len() as i32, glow::UNSIGNED_INT, 0);

        self.gl.bind_vertex_array(None);
    }

    /// Tessellate the accumulated AGG path and draw as a fill with analytic
    /// edge AA.
    ///
    /// The path is flattened and transformed through AGG before
    /// `tessellate_path_aa` attaches a 1-pixel halo strip along every original
    /// polygon boundary.
    pub(crate) unsafe fn do_fill(&mut self) {
        use agg_gui::gl_renderer::tessellate_path_aa;

        let transform = *self.ctm();
        let fill_rule = self.fill_rule;
        let tess = {
            let mut curves = ConvCurve::new(&mut self.path);
            let mut transformed = ConvTransform::new(&mut curves, transform);
            tessellate_path_aa(&mut transformed, 1.0, fill_rule)
        };

        if let Some((verts, idx)) = tess {
            if let Some(gradient) = self.fill_linear_gradient.clone() {
                self.submit_linear_gradient_triangles(&verts, &idx, &gradient, &transform);
            } else if let Some(gradient) = self.fill_radial_gradient.clone() {
                self.submit_radial_gradient_triangles(&verts, &idx, &gradient, &transform);
            } else {
                let color = self.fill_color;
                self.submit_aa_triangles(&verts, &idx, color);
            }
        }
    }

    /// Stroke the accumulated AGG path with analytic edge AA.
    ///
    /// Single battle-tested pipeline:
    ///   AGG `PathStorage` → `ConvStroke` (proper miter/round/bevel joins +
    ///   butt/round/square caps) → [`tessellate_path_aa`] (AGG
    ///   VertexSource → tess2 → interior triangles + edge-flag halo strips).
    pub(crate) unsafe fn do_stroke(&mut self) {
        use agg_gui::gl_renderer::tessellate_path_aa;

        let transform = *self.ctm();
        let width = self.line_width;
        let join = self.line_join;
        let cap = self.line_cap;
        let miter_limit = self.miter_limit;
        let dashes = self.line_dash.clone();
        let dash_offset = self.dash_offset;
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
            let color = self.stroke_color;
            self.submit_aa_triangles(&verts, &idx, color);
        }
    }
}
