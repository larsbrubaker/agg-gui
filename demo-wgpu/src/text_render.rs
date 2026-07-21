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

use crate::{DrawCommand, LcdArcTextureEntry, WgpuGfxCtx};

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

        // Coverage-mask path — AGG rasterises the run to an anti-aliased
        // coverage mask cached at physical (1:1) pixels, then blits it via
        // the 3-pass channel composite. LCD subpixel when `lcd_mode` is on;
        // otherwise a grayscale mask (equal channels, no chroma fringing)
        // for hi-DPI / scaled / touch displays. Both share the same cache,
        // texture upload, and GPU composite — only the finalize differs.
        // The tessellated-outline path below is a last resort for backends
        // that can't composite a coverage mask.
        if self.has_lcd_mask_composite() {
            let phys_size = self.font_size * ctm_scale;
            // LCD subpixel geometry is only valid against the final opaque
            // backbuffer.  Inside a compositing layer we rasterise a grayscale
            // coverage mask instead — it composites (via the flattened,
            // alpha-writing path) without chroma fringing or wash-out.
            let use_lcd = self.lcd_mode && self.layer_stack.is_empty();
            let cached = if use_lcd {
                agg_gui::lcd_coverage::rasterize_text_lcd_cached(&font, text, phys_size)
            } else {
                agg_gui::lcd_coverage::rasterize_text_gray_cached(&font, text, phys_size)
            };
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
        // Masks are immutable, content-addressed CPU rasters — they never
        // mutate in place, so pass `version == 0` to select Arc-identity
        // verification (see `lcd_arc_get_or_upload`).
        let (texture, view) = self.lcd_arc_get_or_upload(mask, 0, mask_w, mask_h);
        self.push_lcd_mask_command(texture, view, mask_w, mask_h, src_color, dst_x, dst_y);
    }

    /// Composite a two-plane LCD backbuffer (colour + alpha planes, both Y-down
    /// RGB8) at `(dst_x, dst_y)` with size `(dst_w, dst_h)`.  Each plane is
    /// cached separately on its `Arc` pointer.
    pub(crate) fn draw_lcd_backbuffer_arc_impl(
        &mut self,
        color: &Arc<Vec<u8>>,
        alpha: &Arc<Vec<u8>>,
        content_version: u64,
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
        // Both planes carry the same `content_version` — an in-place strip edit
        // (`Arc::make_mut`) keeps each plane's buffer address stable while
        // bumping the version, so the cache reuses the texture allocation and
        // re-uploads only when the content actually changed.
        let (color_tex, color_view) = self.lcd_arc_get_or_upload(color, content_version, w, h);
        let (alpha_tex, alpha_view) = self.lcd_arc_get_or_upload(alpha, content_version, w, h);

        // Snap both corners to the integer pixel grid — subpixel phase pattern
        // only valid at 1:1 texel-to-pixel mapping.  BOTH corners must run
        // through the CTM: `dst_w`/`dst_h` are LOGICAL units, so the far corner
        // has to be transformed (not just `bl + dst_w`) or the quad collapses
        // to logical size and the bitmap renders shrunk by `1/ctm_scale` at any
        // scale > 1 (e.g. 110–125% desktop scaling, where LCD is still on).
        let ctm = *self.ctm();
        let far_x = dst_x + dst_w;
        let far_y = dst_y + dst_h;
        let bl_x = (dst_x * ctm.sx + dst_y * ctm.shx + ctm.tx).round();
        let bl_y = (dst_x * ctm.shy + dst_y * ctm.sy + ctm.ty).round();
        let tr_x = (far_x * ctm.sx + far_y * ctm.shx + ctm.tx).round();
        let tr_y = (far_x * ctm.shy + far_y * ctm.sy + ctm.ty).round();

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
            global_alpha: self.global_alpha as f32,
            // Inside a compositing layer the 3-pass premultiplied blit leaves
            // the transparent layer's alpha at 0 (it only writes colour); flatten
            // to a single alpha-writing pass so the pop composite is correct.
            flatten: !self.layer_stack.is_empty(),
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
            // See `LcbMask.flatten` above — inside a layer, render the coverage
            // as a single-pass grayscale quad so the layer accumulates alpha.
            flatten: !self.layer_stack.is_empty(),
        });
    }

    /// Get-or-upload a single `Arc<Vec<u8>>` 3-byte plane into a 4-byte RGBA
    /// texture, keyed on the `Vec`'s heap **buffer address**.
    ///
    /// `version` selects the cache-validity policy (see [`LcdArcTextureEntry`]):
    /// - `version != 0` — a mutable backbuffer plane. The globally-unique
    ///   `content_version` is the source of truth: a matching version means the
    ///   content is unchanged (reuse, no upload); a mismatch means an in-place
    ///   `Arc::make_mut` edit happened, so re-upload into the *existing* texture
    ///   allocation. This is ABA-proof because a recycled buffer address always
    ///   carries a version this entry has never seen.
    /// - `version == 0` — an immutable, content-addressed mask. Those buffers
    ///   never relocate, so validity is checked by Arc pointer identity via the
    ///   stored `weak`.
    pub(crate) fn lcd_arc_get_or_upload(
        &mut self,
        data: &Arc<Vec<u8>>,
        version: u64,
        w: u32,
        h: u32,
    ) -> (Arc<wgpu::Texture>, wgpu::TextureView) {
        // Key on the byte buffer, NOT `Arc::as_ptr`: `Arc::make_mut` relocates
        // the control block on every in-place edit but leaves the buffer put.
        let key = data.as_ref().as_ptr() as usize;

        // Arc-identity is only consulted on the immutable-mask (version == 0)
        // path; skip the upgrade/downgrade churn for the hot backbuffer path.
        let arc_identity_matches = version == 0
            && self
                .lcd_arc_texture_cache
                .get(&key)
                .and_then(|e| e.weak.upgrade())
                .is_some_and(|a| Arc::ptr_eq(&a, data));

        // Decide BEFORE sweeping dead entries: a live backbuffer whose Arc was
        // relocated by `make_mut` leaves a dead `Weak` but a perfectly valid
        // texture — sweeping first would destroy it and force a needless upload.
        let meta = self
            .lcd_arc_texture_cache
            .get(&key)
            .map(|e| LcdEntryMeta { version: e.version, w: e.w, h: e.h });
        let action = lcd_cache_decide(meta, version, w, h, arc_identity_matches);

        let result = match action {
            LcdCacheAction::Reuse => {
                // The stored weak may be dead after a relocation; refresh it so
                // liveness tracking (and the sweep below) stays accurate.
                let e = self
                    .lcd_arc_texture_cache
                    .get_mut(&key)
                    .expect("Reuse implies a present entry");
                e.weak = Arc::downgrade(data);
                (Arc::clone(&e.texture), e.view.clone())
            }
            LcdCacheAction::Rewrite => {
                // Same allocation, changed content: overwrite the existing
                // texture's pixels (full-plane write; row-granular upload is a
                // possible later optimization).
                let (texture, view) = {
                    let e = self
                        .lcd_arc_texture_cache
                        .get(&key)
                        .expect("Rewrite implies a present entry");
                    (Arc::clone(&e.texture), e.view.clone())
                };
                write_lcd_plane(&self.queue, &texture, data.as_slice(), w, h);
                let e = self
                    .lcd_arc_texture_cache
                    .get_mut(&key)
                    .expect("Rewrite implies a present entry");
                e.weak = Arc::downgrade(data);
                e.version = version;
                (texture, view)
            }
            LcdCacheAction::Replace => {
                let (texture, view) =
                    upload_lcd_texture(&self.device, &self.queue, data.as_slice(), w, h);
                self.lcd_arc_texture_cache.insert(
                    key,
                    LcdArcTextureEntry {
                        weak: Arc::downgrade(data),
                        texture: Arc::clone(&texture),
                        view: view.clone(),
                        w,
                        h,
                        version,
                    },
                );
                (texture, view)
            }
        };

        // Sweep dead entries AFTER the touch, but never the entry we just used
        // (its weak may legitimately be dead post-relocation while its texture
        // is live and about to be drawn).
        let dead_keys: Vec<usize> = self
            .lcd_arc_texture_cache
            .iter()
            .filter(|(k, e)| **k != key && e.weak.strong_count() == 0)
            .map(|(k, _)| *k)
            .collect();
        for k in dead_keys {
            self.lcd_arc_texture_cache.remove(&k);
        }

        result
    }
}

/// The three outcomes of an LCD texture-cache lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LcdCacheAction {
    /// Entry is valid — return the existing texture, no GPU upload.
    Reuse,
    /// Entry's allocation is the right size but its content is stale — overwrite
    /// the existing texture's pixels (reusing the allocation).
    Rewrite,
    /// No usable entry (missing or wrong dimensions) — create a new texture.
    Replace,
}

/// The subset of an [`LcdArcTextureEntry`] the pure decision reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LcdEntryMeta {
    pub(crate) version: u64,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

/// Pure hit/miss/version decision for the LCD texture cache, factored out of
/// [`WgpuGfxCtx::lcd_arc_get_or_upload`] so it is unit-testable without a GPU.
///
/// `entry` is the metadata of the entry currently stored under the buffer-
/// address key (or `None` if absent). `arc_identity_matches` is only meaningful
/// on the immutable-mask (`version == 0`) path — the caller sets it from a
/// `Weak::upgrade` + `Arc::ptr_eq` check. Dimensions are always verified: any
/// size change forces [`LcdCacheAction::Replace`].
pub(crate) fn lcd_cache_decide(
    entry: Option<LcdEntryMeta>,
    version: u64,
    w: u32,
    h: u32,
    arc_identity_matches: bool,
) -> LcdCacheAction {
    match entry {
        None => LcdCacheAction::Replace,
        Some(e) if e.w != w || e.h != h => LcdCacheAction::Replace,
        Some(e) => {
            if version != 0 {
                // Mutable backbuffer: version is authoritative.
                if e.version == version {
                    LcdCacheAction::Reuse
                } else {
                    LcdCacheAction::Rewrite
                }
            } else if arc_identity_matches {
                // Immutable mask, same live Arc → texture is still valid.
                LcdCacheAction::Reuse
            } else {
                // Immutable mask whose buffer address was recycled by a new
                // Arc (old one freed): reuse the allocation, upload new content.
                LcdCacheAction::Rewrite
            }
        }
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
    write_lcd_plane(queue, &texture, rgb, w, h);
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (Arc::new(texture), view)
}

/// Write a 3-channel LCD coverage plane into an existing `Rgba8Unorm` texture
/// (RGB→RGBA expanded).  Shared by the fresh-upload and in-place-rewrite paths;
/// the caller guarantees `texture`'s dimensions match `(w, h)`.
fn write_lcd_plane(queue: &wgpu::Queue, texture: &wgpu::Texture, rgb: &[u8], w: u32, h: u32) {
    let rgba = rgb_to_rgba(rgb, w, h);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
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
}
