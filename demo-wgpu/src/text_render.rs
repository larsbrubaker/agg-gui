//! LCD subpixel text rendering for the wgpu backend.
//!
//! Mirrors `demo-gl/src/ctx_core/lcd.rs` and the LCD-related methods in
//! `demo-gl/src/draw_ctx_impl.rs`.  Two flavours:
//!
//! - **LCD mask:** a single 3-channel coverage mask + flat colour.  Used for
//!   freshly-rasterised glyphs and other ad-hoc subpixel content.
//! - **LCD backbuffer:** two cached planes (premultiplied colour + per-channel
//!   alpha) that preserve subpixel chroma through a widget cache round-trip.
//!
//! Both flavours render via the same 3-pass write-mask approach: each colour
//! channel is drawn by its own pipeline (`lcd_r` / `lcd_g` / `lcd_b`) with a
//! `ColorWrites` mask restricting writes to that channel.  This avoids the
//! dual-source-blending GPU feature which is not universally available
//! (notably on the WebGL2 wgpu backend) and matches the WASM path of the GL
//! backend.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::text::shape_glyphs;

use crate::{ArcTextureEntry, DrawCommand, WgpuGfxCtx};

impl WgpuGfxCtx {
    /// Implementation of `DrawCtx::fill_text`.
    ///
    /// Two paths, mirroring `demo-gl/src/text_render.rs::fill_text_impl`:
    /// - **LCD subpixel** when `has_lcd_mask_composite` returns true *and*
    ///   `self.lcd_mode` is on — uses the cached LCD coverage mask path that
    ///   feeds `draw_lcd_mask_arc`.
    /// - **Grayscale outline** otherwise — tessellates glyph outlines via the
    ///   shared `GlyphCache` (XY triangles, no per-vertex alpha) and submits
    ///   them as solid-coloured triangles via `DrawCommand::Solid`.
    pub(crate) fn fill_text_impl(&mut self, text: &str, x: f64, y: f64) {
        let Some(font) = self.font.clone() else {
            return;
        };

        // Extract uniform scale from the CTM — used to render glyph outlines
        // at the *physical* font size on hi-DPI displays.
        let ctm = *self.ctm();
        let ctm_scale = (ctm.sx * ctm.sx + ctm.shy * ctm.shy).sqrt().max(1e-6);

        // LCD subpixel path — same caching strategy as the GL backend.
        if self.has_lcd_mask_composite() && self.lcd_mode {
            let phys_size = self.font_size * ctm_scale;
            let cached = agg_gui::lcd_coverage::rasterize_text_lcd_cached(&font, text, phys_size);
            let mut col = self.fill_color;
            col.a *= self.global_alpha as f32;
            let dst_x = x - cached.baseline_x_in_mask / ctm_scale;
            let dst_y = y - cached.baseline_y_in_mask / ctm_scale;
            self.draw_lcd_mask_arc_impl(
                &cached.pixels,
                cached.width,
                cached.height,
                col,
                dst_x,
                dst_y,
            );
            return;
        }

        // Grayscale outline path.
        let shaped = shape_glyphs(&font, text, self.font_size);
        let font_size = self.font_size;
        let width_scale = agg_gui::font_settings::current_width();
        let italic_shear = agg_gui::font_settings::current_faux_italic() / 3.0;
        let hint_y = agg_gui::font_settings::hinting_enabled();
        let interval_px = agg_gui::font_settings::current_interval() * font_size;
        let tess_size = font_size * ctm_scale;
        let inv_scale = 1.0 / ctm_scale;

        let mut all_verts: Vec<[f32; 2]> = Vec::new();
        let mut all_idx: Vec<u32> = Vec::new();
        let mut pen_x = x;

        for glyph in &shaped {
            let gx = pen_x + glyph.x_offset;
            let gy_raw = y + glyph.y_offset;
            let gy = if hint_y {
                (gy_raw + 0.5).floor()
            } else {
                gy_raw
            };
            let render_font = glyph.fallback_font.as_deref().unwrap_or(&font);

            if let Some(cached) =
                self.glyph_cache
                    .get_or_insert(render_font, glyph.glyph_id, tess_size)
            {
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
            self.commands.push(DrawCommand::Solid {
                verts: all_verts,
                indices: all_idx,
                color: self.fill_color,
                global_alpha: self.global_alpha as f32,
                clip: self.current_clip(),
            });
        }
    }

    /// Slice path for `draw_lcd_mask`.  Uploads a one-shot RGB→RGBA texture and
    /// pushes an `LcdMask` draw command.  Used when no `Arc` is available; the
    /// hot path goes through [`Self::draw_lcd_mask_arc_impl`] below.
    pub(crate) fn draw_lcd_mask_slice_impl(
        &mut self,
        mask: &[u8],
        mask_w: u32,
        mask_h: u32,
        src_color: Color,
        dst_x: f64,
        dst_y: f64,
    ) {
        if mask.is_empty() || mask_w == 0 || mask_h == 0 {
            return;
        }
        if mask.len() < (mask_w as usize) * (mask_h as usize) * 3 {
            return;
        }
        let (texture, view) = upload_lcd_texture(&self.device, &self.queue, mask, mask_w, mask_h);
        self.push_lcd_mask_command(texture, view, mask_w, mask_h, src_color, dst_x, dst_y);
    }

    /// Arc-keyed `draw_lcd_mask` — caches the uploaded texture on the `Arc`'s
    /// pointer identity.  Same lifecycle pattern as the image-blit cache: the
    /// `Weak` ref pins the entry to the pixel buffer's strong-count.
    pub(crate) fn draw_lcd_mask_arc_impl(
        &mut self,
        mask: &Arc<Vec<u8>>,
        mask_w: u32,
        mask_h: u32,
        src_color: Color,
        dst_x: f64,
        dst_y: f64,
    ) {
        if mask.is_empty() || mask_w == 0 || mask_h == 0 {
            return;
        }
        if mask.len() < (mask_w as usize) * (mask_h as usize) * 3 {
            return;
        }
        let (texture, view) = self.lcd_arc_get_or_upload(mask, mask_w, mask_h);
        self.push_lcd_mask_command(texture, view, mask_w, mask_h, src_color, dst_x, dst_y);
    }

    /// Composite a two-plane LCD backbuffer (colour + alpha planes, both Y-down
    /// RGB8) at `(dst_x, dst_y)` with size `(dst_w, dst_h)`.  Each plane is
    /// cached separately on its `Arc` pointer.
    pub(crate) fn draw_lcd_backbuffer_arc_impl(
        &mut self,
        color: &Arc<Vec<u8>>,
        alpha: &Arc<Vec<u8>>,
        w: u32,
        h: u32,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) {
        if w == 0 || h == 0 || color.is_empty() || alpha.is_empty() {
            return;
        }
        let needed = (w as usize) * (h as usize) * 3;
        if color.len() < needed || alpha.len() < needed {
            return;
        }
        let (color_tex, color_view) = self.lcd_arc_get_or_upload(color, w, h);
        let (alpha_tex, alpha_view) = self.lcd_arc_get_or_upload(alpha, w, h);

        // Snap origin to integer pixel grid — subpixel phase pattern only valid
        // at 1:1 texel-to-pixel mapping.
        let ctm = *self.ctm();
        let bl_x = (dst_x * ctm.sx + dst_y * ctm.shx + ctm.tx).round();
        let bl_y = (dst_x * ctm.shy + dst_y * ctm.sy + ctm.ty).round();
        let tr_x = bl_x + dst_w;
        let tr_y = bl_y + dst_h;

        // Cached planes are top-row-first (Y-down image storage), so v=1 at
        // bl (visually-bottom row of the quad samples the last row of data).
        let verts: [f32; 16] = [
            bl_x as f32,
            bl_y as f32,
            0.0,
            1.0,
            tr_x as f32,
            bl_y as f32,
            1.0,
            1.0,
            tr_x as f32,
            tr_y as f32,
            1.0,
            0.0,
            bl_x as f32,
            tr_y as f32,
            0.0,
            0.0,
        ];
        self.commands.push(DrawCommand::LcbMask {
            verts,
            color_tex,
            color_view,
            alpha_tex,
            alpha_view,
            clip: self.current_clip(),
        });
    }

    /// Build the LCD-mask quad verts and push the draw command.
    ///
    /// Origin is snapped to the integer pixel grid for the same reason as
    /// `draw_lcd_backbuffer_arc_impl`: LCD coverage encodes a phased subpixel
    /// pattern at 1:1 texel-to-pixel resolution.
    fn push_lcd_mask_command(
        &mut self,
        texture: Arc<wgpu::Texture>,
        view: wgpu::TextureView,
        mask_w: u32,
        mask_h: u32,
        src_color: Color,
        dst_x: f64,
        dst_y: f64,
    ) {
        let ctm = *self.ctm();
        let bl_x = (dst_x * ctm.sx + dst_y * ctm.shx + ctm.tx).round();
        let bl_y = (dst_x * ctm.shy + dst_y * ctm.sy + ctm.ty).round();
        let tr_x = bl_x + mask_w as f64;
        let tr_y = bl_y + mask_h as f64;

        // Mask rows are Y-up so v=0 maps to the bottom row.
        let verts: [f32; 16] = [
            bl_x as f32,
            bl_y as f32,
            0.0,
            0.0,
            tr_x as f32,
            bl_y as f32,
            1.0,
            0.0,
            tr_x as f32,
            tr_y as f32,
            1.0,
            1.0,
            bl_x as f32,
            tr_y as f32,
            0.0,
            1.0,
        ];

        // Pre-modulate the requested colour by the global alpha so the shader
        // only has to deal with `ch * color.a` once.
        let a = (src_color.a as f64 * self.global_alpha) as f32;
        let color = Color::rgba(src_color.r, src_color.g, src_color.b, a);

        self.commands.push(DrawCommand::LcdMask {
            verts,
            texture,
            view,
            color,
            clip: self.current_clip(),
        });
    }

    /// Get-or-upload a single `Arc<Vec<u8>>` 3-byte plane into a 4-byte RGBA
    /// texture.  Sweeps stale entries (Arc dropped → weak.upgrade fails) on
    /// each call so GPU memory tracks the CPU-side image cache.
    fn lcd_arc_get_or_upload(
        &mut self,
        data: &Arc<Vec<u8>>,
        w: u32,
        h: u32,
    ) -> (Arc<wgpu::Texture>, wgpu::TextureView) {
        let key = Arc::as_ptr(data) as *const u8 as usize;

        // Sweep dead entries.
        let dead_keys: Vec<usize> = self
            .lcd_arc_texture_cache
            .iter()
            .filter(|(_, e)| e.weak.strong_count() == 0)
            .map(|(k, _)| *k)
            .collect();
        for k in dead_keys {
            self.lcd_arc_texture_cache.remove(&k);
        }

        if let Some(entry) = self.lcd_arc_texture_cache.get(&key) {
            if entry.weak.upgrade().is_some() && entry.w == w && entry.h == h {
                return (Arc::clone(&entry.texture), entry.view.clone());
            }
        }

        // Stale or missing — re-upload.
        self.lcd_arc_texture_cache.remove(&key);
        let (texture, view) = upload_lcd_texture(&self.device, &self.queue, data.as_slice(), w, h);
        self.lcd_arc_texture_cache.insert(
            key,
            ArcTextureEntry {
                weak: Arc::downgrade(data),
                texture: Arc::clone(&texture),
                view: view.clone(),
                w,
                h,
            },
        );
        (texture, view)
    }
}

/// Convert tightly-packed RGB8 to RGBA8 (alpha=255).  GPUs don't support
/// 3-byte texture formats; the LCD pipeline shaders sample `.rgb` so the
/// padded alpha byte is harmless.
fn rgb_to_rgba(rgb: &[u8], w: u32, h: u32) -> Vec<u8> {
    let n = (w as usize) * (h as usize);
    let mut out = Vec::with_capacity(n * 4);
    for i in 0..n {
        out.push(rgb[i * 3]);
        out.push(rgb[i * 3 + 1]);
        out.push(rgb[i * 3 + 2]);
        out.push(255);
    }
    out
}

/// Upload a 3-channel LCD coverage plane into a fresh `Rgba8Unorm` texture.
fn upload_lcd_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rgb: &[u8],
    w: u32,
    h: u32,
) -> (Arc<wgpu::Texture>, wgpu::TextureView) {
    let rgba = rgb_to_rgba(rgb, w, h);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (Arc::new(texture), view)
}
