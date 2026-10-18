//! Explicit texture release for [`crate::WgpuGfxCtx`]: the ledger that makes,
//! counts, retires and destroys the context's textures, and the size-keyed
//! pool its transient compositing layers come from (`layers.rs`,
//! `layer_mask.rs`, `comp_op.rs`).
//!
//! Why destroy at all: in the browser, dropping a `wgpu::Texture` does not
//! free its video memory. The browser frees a `GPUTexture` only when its
//! JavaScript garbage collector collects the object, and the collector feels
//! no pressure from video memory, so a canvas resized every frame (a page
//! animating its size) made a new set of surface-sized textures every frame
//! and freed none until a phone's tab ran out of memory. On the desktop wgpu
//! frees on drop, and destroying first frees the same memory a little
//! sooner, so both run the one code path.
//!
//! When: a texture let go may still be used by commands recorded into the
//! frame's encoder, and destroying it before that encoder is submitted makes
//! the submit fail. So a texture the context lets go (a cache eviction, a
//! retained layer rebuilt at a new size, a capture target reallocated, a
//! per-frame gradient ramp or blend-mode copy) goes onto the ledger's retire
//! list, and `flush_to_surface` destroys the list right after the frame's
//! `queue.submit`: everything on it was last used by that frame or an
//! earlier one, and all of those are submitted by then.
//!
//! Layers: a frame's transient layers come from [`LayerPool`]. A layer
//! texture used in one frame is offered again, at the same size, in the next;
//! one no frame asked for is retired. Steady UI therefore allocates no layer
//! textures at all, and a resizing one keeps at most one frame's worth.

use std::sync::Arc;

/// How many textures a [`crate::WgpuGfxCtx`] has made, let go and destroyed,
/// for tests and diagnostics. The ones it still holds (caches, retained
/// layers, the layer pool, capture targets) are [`Self::held`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextureCounts {
    /// Textures the context made.
    pub created: u64,
    /// Textures the context let go and has not destroyed yet (they wait for
    /// the next frame's submit).
    pub retired: u64,
    /// Textures the context made and destroyed.
    pub destroyed: u64,
}

impl TextureCounts {
    /// The textures the context made and still holds.
    pub fn held(&self) -> u64 {
        self.created - self.retired - self.destroyed
    }
}

/// The context's texture ledger: every texture it makes goes through
/// [`Self::create`], every one it lets go through [`Self::retire`].
#[derive(Default)]
pub(crate) struct TextureLedger {
    retired: Vec<wgpu::Texture>,
    /// Textures handed over by [`crate::WgpuGfxCtx::retire_texture`] that the
    /// context did not make; destroyed with the rest but not counted.
    foreign: Vec<wgpu::Texture>,
    created: u64,
    destroyed: u64,
}

impl TextureLedger {
    /// Make the texture `desc` describes, counted.
    pub(crate) fn create(
        &mut self,
        device: &wgpu::Device,
        desc: &wgpu::TextureDescriptor<'_>,
    ) -> wgpu::Texture {
        self.created += 1;
        device.create_texture(desc)
    }

    /// Make a texture that lives for the frame being recorded only: it is
    /// retired at once, so the frame's submit destroys it.
    pub(crate) fn create_for_frame(
        &mut self,
        device: &wgpu::Device,
        desc: &wgpu::TextureDescriptor<'_>,
    ) -> wgpu::Texture {
        let texture = self.create(device, desc);
        self.retire(&texture);
        texture
    }

    /// Let go of a texture this ledger made; it is destroyed after the next
    /// frame's submit. Each texture must be retired once.
    pub(crate) fn retire(&mut self, texture: &wgpu::Texture) {
        self.retired.push(texture.clone());
    }

    /// Let go of a texture the context did not make (see
    /// [`crate::WgpuGfxCtx::retire_texture`]).
    pub(crate) fn retire_foreign(&mut self, texture: wgpu::Texture) {
        self.foreign.push(texture);
    }

    /// Destroy everything retired so far. Called right after a submit: the
    /// encoders that used these textures have all been submitted.
    pub(crate) fn destroy_retired(&mut self) {
        for texture in self.retired.drain(..) {
            texture.destroy();
            self.destroyed += 1;
        }
        for texture in self.foreign.drain(..) {
            texture.destroy();
        }
    }

    pub(crate) fn counts(&self) -> TextureCounts {
        TextureCounts {
            created: self.created,
            retired: self.retired.len() as u64,
            destroyed: self.destroyed,
        }
    }
}

impl Drop for TextureLedger {
    /// A context is dropped between frames (its last frame submitted), so
    /// what it let go can be destroyed now instead of waiting for the
    /// browser's garbage collector.
    fn drop(&mut self) {
        self.destroy_retired();
    }
}

/// One pooled layer texture.
pub(crate) struct PooledLayer {
    pub(crate) texture: Arc<wgpu::Texture>,
    pub(crate) view: wgpu::TextureView,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Transient layer textures, reused frame to frame by exact size.
///
/// A texture is never handed out twice within one frame: the frame's
/// commands run later, in recording order, and two layers sharing a texture
/// would overwrite each other. Reuse is safe across frames because the queue
/// runs a frame's work after the previous frame's, and every push clears its
/// layer (`LoadOp::Clear`), so nothing from the earlier use shows through.
#[derive(Default)]
pub(crate) struct LayerPool {
    /// Used by the previous frame, free for this one.
    free: Vec<PooledLayer>,
    /// Handed out in this frame.
    in_use: Vec<PooledLayer>,
}

impl LayerPool {
    /// A `w` x `h` layer for this frame: last frame's at that size when
    /// there is one, otherwise a new texture from `make`.
    pub(crate) fn take(
        &mut self,
        w: u32,
        h: u32,
        make: impl FnOnce() -> (Arc<wgpu::Texture>, wgpu::TextureView),
    ) -> (Arc<wgpu::Texture>, wgpu::TextureView) {
        let layer = match self.free.iter().position(|l| l.width == w && l.height == h) {
            Some(i) => self.free.swap_remove(i),
            None => {
                let (texture, view) = make();
                PooledLayer {
                    texture,
                    view,
                    width: w,
                    height: h,
                }
            }
        };
        let handed = (Arc::clone(&layer.texture), layer.view.clone());
        self.in_use.push(layer);
        handed
    }

    /// End of a frame (after its submit): textures no frame asked for are
    /// retired to `ledger`; this frame's become next frame's free ones.
    pub(crate) fn end_frame(&mut self, ledger: &mut TextureLedger) {
        for unused in self.free.drain(..) {
            ledger.retire(&unused.texture);
        }
        std::mem::swap(&mut self.free, &mut self.in_use);
    }
}
