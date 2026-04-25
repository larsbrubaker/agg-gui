use super::*;

impl GlGfxCtx {
    pub(crate) fn fill_text_impl(&mut self, text: &str, x: f64, y: f64) {
        let font = match self.font.clone() {
            Some(f) => f,
            None => return,
        };

        // Extract uniform scale from the CTM — used below to render text at
        // **physical** font size rather than logical.  Pure scale transforms
        // (what we use for DPI) give `sqrt(sx² + shy²) == scale`.
        let ctm = *self.ctm();
        let ctm_scale = (ctm.sx * ctm.sx + ctm.shy * ctm.shy).sqrt().max(1e-6);

        // Y-baseline alignment between LCD and grayscale paths is
        // controlled by the hinting toggle.  When hinting is on, both
        // paths snap to the same integer physical pixel row (RGBA via
        // `shape_text`'s `gy` snap, LCD via the in-mask `by` snap in
        // `rasterize_text_lcd_cached`).  When off, the RGBA path
        // accepts exact fractional `y` while the LCD composite's
        // intrinsic row-alignment leaves a tiny residual offset —
        // see the comment in `gfx_ctx::fill_text` for the full story.

        // LCD subpixel path — raster is cached in `text_lcd` keyed on
        // `(text, font, size)`; the GL backend then caches the uploaded
        // texture keyed on the returned `Arc`'s pointer identity via
        // `draw_lcd_mask_arc`.  Result: one AGG rasterisation +
        // `glTexImage2D` per unique string, every subsequent frame is a
        // single dual-source-blend draw call.
        //
        // HiDPI: rasterise at physical size so the mask composites 1:1 at
        // the right pixel count instead of being half-sized on 2×/3× screens.
        if <Self as agg_gui::DrawCtx>::has_lcd_mask_composite(self) && self.lcd_mode {
            let phys_size = self.font_size * ctm_scale;
            let cached = agg_gui::lcd_coverage::rasterize_text_lcd_cached(&font, text, phys_size);
            let mut col = self.fill_color;
            col.a *= self.global_alpha as f32;
            // `baseline_*_in_mask` is in physical mask pixels; divide by
            // `ctm_scale` so offsets stay in logical units that the CTM
            // inside `draw_lcd_mask_arc` multiplies back to physical.
            let dst_x = x - cached.baseline_x_in_mask / ctm_scale;
            let dst_y = y - cached.baseline_y_in_mask / ctm_scale;
            <Self as agg_gui::DrawCtx>::draw_lcd_mask_arc(
                self,
                &cached.pixels,
                cached.width,
                cached.height,
                col,
                dst_x,
                dst_y,
            );
            return;
        }

        // Shape the text string to get per-glyph IDs and advances.
        // Rustybuzz shaping is cheap relative to tessellation.
        let shaped = shape_glyphs(&font, text, self.font_size);
        let font_size = self.font_size;

        // Typography-style globals consulted per-frame (scrollbar-style
        // pattern).  These piggy-back onto the existing GL glyph cache
        // — the cache stores native-shape outlines at origin; width /
        // italic are applied vertex-by-vertex below, hinting snaps the
        // Y origin, and interval pads the pen advance.
        let width_scale = agg_gui::font_settings::current_width();
        let italic_shear = agg_gui::font_settings::current_faux_italic() / 3.0;
        let hint_y = agg_gui::font_settings::hinting_enabled();
        let interval_px = agg_gui::font_settings::current_interval() * font_size;
        // HiDPI: cache glyph tessellations at the **physical** size so the
        // Bezier flattening resolves more segments on 2×/3× displays.  We
        // then divide each vertex by `ctm_scale` before adding the glyph
        // origin, so positions stay in logical units — the outer CTM
        // transforms them back to physical pixels for the GPU.  Net: same
        // on-screen size as before, but curves are tessellated at the
        // higher resolution.
        let tess_size = font_size * ctm_scale;
        let inv_scale = 1.0 / ctm_scale;

        let mut all_verts: Vec<[f32; 2]> = Vec::new();
        let mut all_idx: Vec<u32> = Vec::new();
        let mut pen_x = x;

        for glyph in &shaped {
            // Glyph origin in widget-local pixel space (before CTM).
            let gx = pen_x + glyph.x_offset;
            let gy_raw = y + glyph.y_offset;
            // Y-axis hinting: snap baseline to the pixel grid.  The X
            // coordinate keeps its subpixel precision, which matters
            // for LCD positioning and smooth scrolling.
            let gy = if hint_y {
                (gy_raw + 0.5).floor()
            } else {
                gy_raw
            };

            // Use the fallback font for outline lookup when the glyph was
            // resolved from it — glyph_id is an index into that font's table.
            let render_font = glyph.fallback_font.as_deref().unwrap_or(&font);

            if let Some(cached) =
                self.glyph_cache
                    .get_or_insert(render_font, glyph.glyph_id, tess_size)
            {
                // Vertices are in physical pixel space at `tess_size`.  Scale
                // to logical via `inv_scale`, offset by the glyph's logical
                // pen position, then apply the CTM to reach physical pixels.
                //
                // Width scale multiplies the X contribution; faux italic
                // adds `y * italic_shear` to X (matching the agg-rust
                // `TransAffine::new_skewing(faux_italic/3, 0)` convention
                // — the `/3` already happened in `italic_shear` above).
                // The cached Y stays native, so the cache doesn't need to
                // track the new style parameters.
                let base = all_verts.len() as u32;
                for &[vx, vy] in &cached.verts {
                    let vx_f64 = vx as f64 * inv_scale;
                    let vy_f64 = vy as f64 * inv_scale;
                    let (mut px, mut py) = (
                        gx + vx_f64 * width_scale + vy_f64 * italic_shear,
                        gy + vy_f64,
                    );
                    ctm.transform(&mut px, &mut py);
                    all_verts.push([px as f32, py as f32]);
                }
                all_idx.extend(cached.indices.iter().map(|&i| i + base));
            }

            pen_x += glyph.x_advance + interval_px;
        }

        if !all_verts.is_empty() {
            let color = self.fill_color;
            unsafe {
                self.draw_triangles(&all_verts, &all_idx, color);
            }
        }
    }
}
