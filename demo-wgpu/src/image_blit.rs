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
    /// Build the 6-vertex (2 triangles) textured-quad vertex buffer for
    /// `(dst_x, dst_y, dst_w, dst_h)` in local coordinates, transformed through
    /// the current CTM.  UV `v` runs top→bottom (top of the destination quad
    /// samples `v=0`) to match the Y-down image convention used by callers.
    fn build_image_verts(
        &self,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) -> [f32; 24] {
        let bl = self.transform_pt(dst_x, dst_y);
        let br = self.transform_pt(dst_x + dst_w, dst_y);
        let tr = self.transform_pt(dst_x + dst_w, dst_y + dst_h);
        let tl = self.transform_pt(dst_x, dst_y + dst_h);
        [
            bl[0], bl[1], 0.0, 1.0,
            br[0], br[1], 1.0, 1.0,
            tr[0], tr[1], 1.0, 0.0,
            bl[0], bl[1], 0.0, 1.0,
            tr[0], tr[1], 1.0, 0.0,
            tl[0], tl[1], 0.0, 0.0,
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
            let (texture, view) = upload_rgba_texture(&self.device, &self.queue, data, img_w, img_h);
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

        self.commands.push(DrawCommand::Textured {
            verts,
            texture,
            view,
            nearest: false,
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
                Some(a) if Arc::ptr_eq(&a, data) && e.w == img_w && e.h == img_h => Some((
                    Arc::clone(&e.texture),
                    e.view.clone(),
                )),
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

        self.commands.push(DrawCommand::Textured {
            verts,
            texture,
            view,
            nearest: true,
            clip: self.current_clip(),
        });
    }
}

/// Upload an RGBA8 image to a freshly-created `wgpu::Texture` and return both
/// the `Arc<Texture>` and its default view.  Used by both the slice and arc
/// blit paths; the texture is always created with `TEXTURE_BINDING | COPY_DST`.
fn upload_rgba_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &[u8],
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
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (Arc::new(texture), view)
}
