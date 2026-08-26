//! Texture upload and image blit helpers for the wgpu backend.
//!
//! Mirrors `demo-gl/src/image_blit.rs`, providing:
//! - [`WgpuGfxCtx::draw_image_rgba_slice_impl`] — uploads a `&[u8]` as a
//!   transient texture keyed by a lightweight hash, with LRU eviction at 512
//!   entries.  Used by callers that hand us a borrowed pixel slice.
//! - [`WgpuGfxCtx::draw_image_rgba_arc_impl`] — Arc-pointer-keyed hot path for
//!   `Label` backbuffers; one GPU upload per unique raster, lifetime tied to
//!   the `Arc<Vec<u8>>` strong-count via a `Weak` sentinel.

use std::sync::Arc;

use crate::{ArcTextureEntry, DrawCommand, WgpuGfxCtx};

/// Maximum entries in the slice-keyed texture cache before LRU eviction kicks in.
const TEX_CACHE_MAX: usize = 512;

/// Compute a lightweight cache key for an RGBA image slice.
///
/// Mirrors `demo-gl/src/gl_support.rs::texture_key`.  Blends pointer, length,
/// dimensions, and head/tail bytes — cheap, no full-buffer hash.
pub(crate) fn texture_key(data: &[u8], w: u32, h: u32) -> u64 {
    let mut k: u64 = 0xcbf29ce484222325;
    let mix = |acc: u64, v: u64| -> u64 { acc.wrapping_mul(0x100000001b3).wrapping_add(v) };
    k = mix(k, data.as_ptr() as usize as u64);
    k = mix(k, data.len() as u64);
    k = mix(k, w as u64);
    k = mix(k, h as u64);
    if data.len() >= 16 {
        for &b in &data[..8] {
            k = mix(k, b as u64);
        }
        for &b in &data[data.len() - 8..] {
            k = mix(k, b as u64);
        }
    } else {
        for &b in data {
            k = mix(k, b as u64);
        }
    }
    k
}

impl WgpuGfxCtx {
    /// Build a 6-vertex textured-quad vertex buffer from four explicit
    /// destination corners in local Y-up coordinates. Each corner is
    /// run through the current CTM. Order: bottom-left, bottom-right,
    /// top-right, top-left. UV mapping is fixed: BL=(0,1), BR=(1,1),
    /// TR=(1,0), TL=(0,0).
    pub(crate) fn build_image_verts_corners(&self, corners: [(f64, f64); 4]) -> [f32; 24] {
        let bl = self.transform_pt(corners[0].0, corners[0].1);
        let br = self.transform_pt(corners[1].0, corners[1].1);
        let tr = self.transform_pt(corners[2].0, corners[2].1);
        let tl = self.transform_pt(corners[3].0, corners[3].1);
        [
            bl[0], bl[1], 0.0, 1.0, br[0], br[1], 1.0, 1.0, tr[0], tr[1], 1.0, 0.0, bl[0], bl[1],
            0.0, 1.0, tr[0], tr[1], 1.0, 0.0, tl[0], tl[1], 0.0, 0.0,
        ]
    }

    /// Build the 6-vertex (2 triangles) textured-quad vertex buffer for
    /// `(dst_x, dst_y, dst_w, dst_h)` in local coordinates, transformed through
    /// the current CTM.  UV `v` runs top→bottom (top of the destination quad
    /// samples `v=0`) to match the Y-down image convention used by callers.
    fn build_image_verts(&self, dst_x: f64, dst_y: f64, dst_w: f64, dst_h: f64) -> [f32; 24] {
        let bl = self.transform_pt(dst_x, dst_y);
        let br = self.transform_pt(dst_x + dst_w, dst_y);
        let tr = self.transform_pt(dst_x + dst_w, dst_y + dst_h);
        let tl = self.transform_pt(dst_x, dst_y + dst_h);
        [
            bl[0], bl[1], 0.0, 1.0, br[0], br[1], 1.0, 1.0, tr[0], tr[1], 1.0, 0.0, bl[0], bl[1],
            0.0, 1.0, tr[0], tr[1], 1.0, 0.0, tl[0], tl[1], 0.0, 0.0,
        ]
    }

    /// Slice-keyed image blit.  Use the lightweight texture-cache key to reuse
    /// uploads across frames; LRU-evict at [`TEX_CACHE_MAX`] entries.
    ///
    /// Sampler: linear (smooth scaling for non-1:1 blits).
    pub(crate) fn draw_image_rgba_slice_impl(
        &mut self,
        data: &[u8],
        img_w: u32,
        img_h: u32,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) {
        if img_w == 0 || img_h == 0 || dst_w <= 0.0 || dst_h <= 0.0 {
            return;
        }
        if data.len() < (img_w as usize) * (img_h as usize) * 4 {
            return;
        }

        let verts = self.build_image_verts(dst_x, dst_y, dst_w, dst_h);
        let key = texture_key(data, img_w, img_h);

        let (texture, view) = if let Some(entry) = self.texture_cache.get(&key).cloned() {
            // LRU touch — move key to back of order.
            if let Some(pos) = self.texture_cache_order.iter().position(|&k| k == key) {
                self.texture_cache_order.remove(pos);
            }
            self.texture_cache_order.push_back(key);
            (entry.0, entry.1)
        } else {
            let (texture, view) =
                upload_rgba_texture(&self.device, &self.queue, data, img_w, img_h);
            self.texture_cache
                .insert(key, (Arc::clone(&texture), view.clone(), img_w, img_h));
            self.texture_cache_order.push_back(key);
            // LRU evict to cap.
            while self.texture_cache.len() > TEX_CACHE_MAX {
                if let Some(old_key) = self.texture_cache_order.pop_front() {
                    self.texture_cache.remove(&old_key);
                } else {
                    break;
                }
            }
            (texture, view)
        };

        let alpha = self.global_alpha as f32;
        self.commands.push(DrawCommand::Textured {
            verts,
            texture,
            view,
            nearest: false,
            tint: [1.0, 1.0, 1.0, alpha],
            clip: self.current_clip(),
        });
    }

    /// Arc-keyed image blit (the `Label` backbuffer hot path).
    ///
    /// Cache lifetime is tied to the `Arc<Vec<u8>>` strong-count: when all
    /// strong refs to the underlying pixel buffer are dropped (typically because
    /// the L1 image cache evicted the entry), the cached entry's `Weak` ref
    /// fails to upgrade and the entry is swept on the next call.
    ///
    /// Sampler: nearest (preserves crisp 1:1 backbuffers — the same rationale
    /// as the GL backend; LINEAR yields driver-dependent fuzz on integer-aligned
    /// labels because of sub-texel rounding differences).
    pub(crate) fn draw_image_rgba_arc_impl(
        &mut self,
        data: &Arc<Vec<u8>>,
        img_w: u32,
        img_h: u32,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) {
        if img_w == 0 || img_h == 0 || dst_w <= 0.0 || dst_h <= 0.0 {
            return;
        }
        if data.len() < (img_w as usize) * (img_h as usize) * 4 {
            return;
        }

        let verts = self.build_image_verts(dst_x, dst_y, dst_w, dst_h);
        let key = Arc::as_ptr(data) as *const u8 as usize;

        // Sweep dead entries (Arc dropped → weak.upgrade fails).  Bounded by
        // L1 cache cap so the scan stays cheap.
        let dead_keys: Vec<usize> = self
            .arc_texture_cache
            .iter()
            .filter(|(_, e)| e.weak.strong_count() == 0)
            .map(|(k, _)| *k)
            .collect();
        for k in dead_keys {
            self.arc_texture_cache.remove(&k);
        }

        // Look up by pointer + verify via Weak::upgrade so a recycled pointer
        // (old entry died, new Arc happened to allocate at the same address)
        // doesn't return a stale texture.
        let existing = self
            .arc_texture_cache
            .get(&key)
            .and_then(|e| match e.weak.upgrade() {
                Some(a) if Arc::ptr_eq(&a, data) && e.w == img_w && e.h == img_h => {
                    Some((Arc::clone(&e.texture), e.view.clone()))
                }
                _ => None,
            });

        let (texture, view) = match existing {
            Some(t) => t,
            None => {
                self.arc_texture_cache.remove(&key);
                let (texture, view) =
                    upload_rgba_texture(&self.device, &self.queue, data.as_slice(), img_w, img_h);
                self.arc_texture_cache.insert(
                    key,
                    ArcTextureEntry {
                        weak: Arc::downgrade(data),
                        texture: Arc::clone(&texture),
                        view: view.clone(),
                        w: img_w,
                        h: img_h,
                    },
                );
                (texture, view)
            }
        };

        // Sampler choice tracks the upload's mipmap policy:
        // - Small textures (Label backbuffers, glyph atlases): no mipmaps
        //   were generated, so use the nearest sampler — preserves the
        //   crisp 1:1 pixel-perfect blit the Label / pixel-test paths
        //   were originally written for.
        // - Large textures (screenshots, big images): mipmap chain was
        //   uploaded; use the linear (trilinear-with-mipmaps) sampler so
        //   downsampling at draw time picks an appropriate mip level
        //   instead of point-aliasing at the base mip.
        let use_nearest = !should_use_mipmaps(img_w, img_h);
        let alpha = self.global_alpha as f32;
        self.commands.push(DrawCommand::Textured {
            verts,
            texture,
            view,
            nearest: use_nearest,
            tint: [1.0, 1.0, 1.0, alpha],
            clip: self.current_clip(),
        });
    }

    /// Blit `data` as a textured quad whose four destination corners
    /// are supplied explicitly. Drives perspective-projected card
    /// flip animations: the caller passes the four projected screen-
    /// space corners of a 3-D rotated card, and we render it as a
    /// real (potentially trapezoidal) textured quad rather than an
    /// axis-aligned blit. Uses the Arc-pointer-keyed texture cache
    /// so per-frame uploads don't recur.
    pub(crate) fn draw_image_rgba_corners_impl(
        &mut self,
        data: &Arc<Vec<u8>>,
        img_w: u32,
        img_h: u32,
        corners: [(f64, f64); 4],
    ) {
        if img_w == 0 || img_h == 0 {
            return;
        }
        if data.len() < (img_w as usize) * (img_h as usize) * 4 {
            return;
        }
        let verts = self.build_image_verts_corners(corners);
        let key = Arc::as_ptr(data) as *const u8 as usize;

        let dead_keys: Vec<usize> = self
            .arc_texture_cache
            .iter()
            .filter(|(_, e)| e.weak.strong_count() == 0)
            .map(|(k, _)| *k)
            .collect();
        for k in dead_keys {
            self.arc_texture_cache.remove(&k);
        }

        let existing = self
            .arc_texture_cache
            .get(&key)
            .and_then(|e| match e.weak.upgrade() {
                Some(a) if Arc::ptr_eq(&a, data) && e.w == img_w && e.h == img_h => {
                    Some((Arc::clone(&e.texture), e.view.clone()))
                }
                _ => None,
            });

        let (texture, view) = match existing {
            Some(t) => t,
            None => {
                self.arc_texture_cache.remove(&key);
                let (texture, view) =
                    upload_rgba_texture(&self.device, &self.queue, data.as_slice(), img_w, img_h);
                self.arc_texture_cache.insert(
                    key,
                    ArcTextureEntry {
                        weak: Arc::downgrade(data),
                        texture: Arc::clone(&texture),
                        view: view.clone(),
                        w: img_w,
                        h: img_h,
                    },
                );
                (texture, view)
            }
        };

        let use_nearest = !should_use_mipmaps(img_w, img_h);
        let alpha = self.global_alpha as f32;
        self.commands.push(DrawCommand::Textured {
            verts,
            texture,
            view,
            nearest: use_nearest,
            tint: [1.0, 1.0, 1.0, alpha],
            clip: self.current_clip(),
        });
    }
}

/// Threshold at which an upload generates a mipmap chain so heavily
/// downsampled draws (e.g. the screenshot preview pane shrinking a
/// 1920×1080 capture into a 400×225 panel) sample with trilinear
/// quality instead of point-aliasing.
///
/// Smaller textures (Label backbuffers, glyph images) are typically
/// drawn at their native size and benefit from no-mipmap, point-filter
/// crispness.
const MIPMAP_MIN_DIM: u32 = 256;

/// Whether an image of `(w, h)` should get a mipmap chain on upload.
pub(crate) fn should_use_mipmaps(w: u32, h: u32) -> bool {
    w.max(h) >= MIPMAP_MIN_DIM
}

/// Upload an RGBA8 image to a freshly-created `wgpu::Texture` and return both
/// the `Arc<Texture>` and its default view.  Used by both the slice and arc
/// blit paths; the texture is always created with `TEXTURE_BINDING | COPY_DST`.
///
/// Generates a CPU-side box-filtered mipmap chain when [`should_use_mipmaps`]
/// returns true.  The mips are cheap one-shot work at upload (screenshots
/// upload rarely; the L1 image cache reuses the texture across frames) and
/// give the linear+linear+linear-mipmap sampler enough levels to produce a
/// clean filtered result no matter how aggressively the call site
/// downsamples — a 4× shrink samples mip 2, a 16× shrink samples mip 4, etc.
fn upload_rgba_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &[u8],
    w: u32,
    h: u32,
) -> (Arc<wgpu::Texture>, wgpu::TextureView) {
    let with_mipmaps = should_use_mipmaps(w, h);
    let mip_level_count = if with_mipmaps {
        // floor(log2(max(w, h))) + 1 levels — the WebGPU rule for a full
        // chain down to the 1×1 root mip.
        w.max(h).max(1).ilog2() + 1
    } else {
        1
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    // Mip 0: the source bytes, full resolution.
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
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

    // Mips 1..N: each level is the previous level box-filtered to half the
    // dimensions.  Box filter (averaging 2×2 blocks) is the standard
    // mip-generation kernel — Lanczos / Mitchell would be sharper but the
    // visual difference at typical screenshot panel sizes is negligible
    // and the box filter is trivially correct.  Done CPU-side because
    // screenshots upload rarely; a GPU compute / blit chain would be
    // faster but pulls in a render pass per level.
    if with_mipmaps {
        let mut prev = data.to_vec();
        let mut prev_w = w;
        let mut prev_h = h;
        for level in 1..mip_level_count {
            let next_w = (prev_w / 2).max(1);
            let next_h = (prev_h / 2).max(1);
            let next = box_downsample(&prev, prev_w, prev_h, next_w, next_h);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &next,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(next_w * 4),
                    rows_per_image: Some(next_h),
                },
                wgpu::Extent3d {
                    width: next_w,
                    height: next_h,
                    depth_or_array_layers: 1,
                },
            );
            prev = next;
            prev_w = next_w;
            prev_h = next_h;
        }
    }

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (Arc::new(texture), view)
}

/// Box-filter a source RGBA8 image to the given target size, averaging
/// each `(src_w/dst_w) × (src_h/dst_h)` block into a single pixel.  Used
/// for CPU mip-chain generation in [`upload_rgba_texture`].
fn box_downsample(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let mut dst = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
    // The typical case is dst dims = src/2; we keep the math general for
    // the odd-pixel rounding case (e.g. a 17×17 → 8×8 step where the last
    // row/col needs to average a 3-row block).
    let kx = src_w as f32 / dst_w as f32;
    let ky = src_h as f32 / dst_h as f32;
    for dy in 0..dst_h {
        for dx in 0..dst_w {
            let sx0 = (dx as f32 * kx).floor() as u32;
            let sx1 = ((dx + 1) as f32 * kx).ceil().min(src_w as f32) as u32;
            let sy0 = (dy as f32 * ky).floor() as u32;
            let sy1 = ((dy + 1) as f32 * ky).ceil().min(src_h as f32) as u32;
            let mut r = 0u32;
            let mut g = 0u32;
            let mut b = 0u32;
            let mut a = 0u32;
            let mut n = 0u32;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let i = ((sy * src_w + sx) * 4) as usize;
                    r += src[i] as u32;
                    g += src[i + 1] as u32;
                    b += src[i + 2] as u32;
                    a += src[i + 3] as u32;
                    n += 1;
                }
            }
            if n > 0 {
                let di = ((dy * dst_w + dx) * 4) as usize;
                dst[di] = (r / n) as u8;
                dst[di + 1] = (g / n) as u8;
                dst[di + 2] = (b / n) as u8;
                dst[di + 3] = (a / n) as u8;
            }
        }
    }
    dst
}
