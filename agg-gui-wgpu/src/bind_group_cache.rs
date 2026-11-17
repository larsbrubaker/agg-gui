//! The bind groups a [`crate::WgpuGfxCtx`] binds in its frames: made once,
//! reused by every later frame that draws the same thing, let go when what
//! they bind goes, and counted ([`BindGroupCounts`]).
//!
//! Why reuse them: in Firefox a bind group is freed only when JavaScript's
//! garbage collector collects its wrapper, and on D3D12 each one takes its
//! descriptors from a fixed, shader-visible heap of about a million per
//! device. Dropping a `wgpu::BindGroup` frees nothing until the collector
//! runs, and it feels no pressure from that heap: a UI that made a few
//! hundred bind groups every frame lost its device ("Out of memory") after a
//! minute or two of redraws. So a steady frame makes none.
//!
//! Group 0, the uniforms: each draw's uniforms are a slice of a chunk of the
//! frame's uniform arena (`buffer_arena.rs`), and the chunks persist from
//! frame to frame. Every uniform layout takes a dynamic offset
//! (`bind_group_layouts.rs`), so one bind group per layout and chunk, binding
//! the chunk's first `size` bytes, serves every draw of that layout in that
//! chunk: the draw passes its slice's offset to `set_bind_group`
//! ([`UniformBind`]). The arena aligns each allocation to
//! `min_uniform_buffer_offset_alignment`, which a dynamic offset must be a
//! multiple of, and keeps it inside its chunk. The bind group goes when the
//! arena no longer holds its chunk (a chunk replaced by a larger one).
//!
//! Group 1, the textures: a bind group per layout, texture view(s) and
//! sampler. The views come from textures that outlive a frame (image and LCD
//! caches, layer textures from the pool, retained layers, the screenshot
//! capture), each made once with its texture and cloned into every draw, so
//! the next frame brings back the same view. A bind group no frame used is
//! let go at that frame's end, as `LayerPool` lets go of an unused layer
//! (`texture_life.rs`), and so is one whose texture the frame let go of, so
//! the cache never keeps a texture alive past the submit that destroys it.
//!
//! What binds a texture made for one frame (a gradient's ramp, a blend-mode
//! composite's destination copy, a screenshot blit) makes its bind group
//! with [`BindGroupCache::one_frame`]: counted, not kept. An LCD mask drawn
//! from a slice rather than an `Arc` is the one one-frame texture on the
//! cached path (it shares the draw command of the cached masks); its texture
//! is let go in its frame, so its bind group goes at that frame's end.
//!
//! The keys hold clones of what they bind: wgpu resources compare and hash by
//! identity, and a resource still held cannot hand its identity to a new one.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::buffer_arena::GpuArena;
use crate::end_frame::PreparedSlice;
use crate::texture_life::TextureLedger;

/// How many bind groups a [`crate::WgpuGfxCtx`] has made, by kind, and how
/// many it keeps for later frames, for tests and diagnostics. After the first
/// frames of a steady UI, only [`Self::one_frame`] grows, by the frame's
/// one-frame textures; a `uniform` or `texture` count that grows every
/// frame is the context making bind groups it should reuse.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BindGroupCounts {
    /// Group-0 bind groups over a chunk of the frame's uniform buffer, one
    /// per uniform layout and chunk.
    pub uniform: u64,
    /// Group-1 bind groups over textures that outlive a frame (including the
    /// one over the AA step texture, made with the context, and those over
    /// LCD masks drawn from a slice, which last one frame).
    pub texture: u64,
    /// Bind groups made for one frame only (a gradient's ramp, a blend-mode
    /// composite, a scaled screenshot).
    pub one_frame: u64,
    /// `uniform` and `texture` bind groups the context keeps for later
    /// frames now (the AA step texture's, held for the context's life, aside).
    pub cached: u64,
}

impl BindGroupCounts {
    /// Every bind group the context has made.
    pub fn created(&self) -> u64 {
        self.uniform + self.texture + self.one_frame
    }
}

/// Which count a bind group goes into.
#[derive(Clone, Copy)]
pub(crate) enum BindGroupKind {
    Uniform,
    Texture,
    OneFrame,
}

/// A draw's group 0: the bind group over its uniform arena chunk and the
/// dynamic offset of its slice in that chunk.
#[derive(Clone)]
pub(crate) struct UniformBind {
    pub(crate) group: Arc<wgpu::BindGroup>,
    pub(crate) offset: u32,
}

impl UniformBind {
    /// Bind as group 0 of `pass`.
    pub(crate) fn set(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &*self.group, &[self.offset]);
    }
}

#[derive(PartialEq, Eq, Hash)]
struct UniformKey {
    layout: wgpu::BindGroupLayout,
    chunk: wgpu::Buffer,
    /// The layout's uniform struct size: the bound range.
    size: u64,
}

#[derive(PartialEq, Eq, Hash)]
struct TextureKey {
    layout: wgpu::BindGroupLayout,
    view: wgpu::TextureView,
    /// The LCD backbuffer's alpha plane; `None` for one-texture layouts.
    second_view: Option<wgpu::TextureView>,
    sampler: wgpu::Sampler,
}

struct TextureEntry {
    group: Arc<wgpu::BindGroup>,
    /// Bound by the frame being recorded.
    used: bool,
}

/// The context's bind groups kept for later frames, and the counts of every
/// one it made. See the module docs.
#[derive(Default)]
pub(crate) struct BindGroupCache {
    uniform: HashMap<UniformKey, Arc<wgpu::BindGroup>>,
    texture: HashMap<TextureKey, TextureEntry>,
    counts: BindGroupCounts,
}

impl BindGroupCache {
    /// Make a bind group, counted as `kind`.
    pub(crate) fn create(
        &mut self,
        kind: BindGroupKind,
        device: &wgpu::Device,
        desc: &wgpu::BindGroupDescriptor<'_>,
    ) -> wgpu::BindGroup {
        match kind {
            BindGroupKind::Uniform => self.counts.uniform += 1,
            BindGroupKind::Texture => self.counts.texture += 1,
            BindGroupKind::OneFrame => self.counts.one_frame += 1,
        }
        device.create_bind_group(desc)
    }

    /// Make a bind group over something made for this frame only.
    pub(crate) fn one_frame(
        &mut self,
        device: &wgpu::Device,
        desc: &wgpu::BindGroupDescriptor<'_>,
    ) -> wgpu::BindGroup {
        self.create(BindGroupKind::OneFrame, device, desc)
    }

    /// Group 0 for a draw whose uniforms are `slice` of the uniform arena,
    /// under `layout` (a dynamic-offset uniform layout whose struct is
    /// `slice.size` bytes).
    pub(crate) fn uniform(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        slice: &PreparedSlice,
    ) -> UniformBind {
        // A uniform chunk is far smaller than 4 GiB (256 KiB unless a single
        // allocation needs more, and uniform structs are tens of bytes), so
        // every offset in it fits the `u32` a dynamic offset is.
        debug_assert!(slice.offset <= u32::MAX as u64);
        let key = UniformKey {
            layout: layout.clone(),
            chunk: (*slice.buf).clone(),
            size: slice.size,
        };
        let group = match self.uniform.get(&key) {
            Some(group) => Arc::clone(group),
            None => {
                let group = Arc::new(self.create(
                    BindGroupKind::Uniform,
                    device,
                    &wgpu::BindGroupDescriptor {
                        label: None,
                        layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            // The struct's size, not the rest of the chunk:
                            // the dynamic offset moves this window over the
                            // chunk, and offset + size must stay inside it.
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &slice.buf,
                                offset: 0,
                                size: wgpu::BufferSize::new(slice.size),
                            }),
                        }],
                    },
                ));
                self.uniform.insert(key, Arc::clone(&group));
                group
            }
        };
        UniformBind {
            group,
            offset: slice.offset as u32,
        }
    }

    /// Group 1 binding `views` (one, or the LCD backbuffer's two planes) at
    /// bindings 0.. and `sampler` after them, under `layout`. The views must
    /// belong to textures that outlive the frame.
    pub(crate) fn texture(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        view: &wgpu::TextureView,
        second_view: Option<&wgpu::TextureView>,
        sampler: &wgpu::Sampler,
    ) -> Arc<wgpu::BindGroup> {
        let key = TextureKey {
            layout: layout.clone(),
            view: view.clone(),
            second_view: second_view.cloned(),
            sampler: sampler.clone(),
        };
        if let Some(entry) = self.texture.get_mut(&key) {
            entry.used = true;
            return Arc::clone(&entry.group);
        }
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(view),
        }];
        if let Some(second) = second_view {
            entries.push(wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(second),
            });
        }
        entries.push(wgpu::BindGroupEntry {
            binding: entries.len() as u32,
            resource: wgpu::BindingResource::Sampler(sampler),
        });
        let group = Arc::new(self.create(
            BindGroupKind::Texture,
            device,
            &wgpu::BindGroupDescriptor {
                label: None,
                layout,
                entries: &entries,
            },
        ));
        self.texture.insert(
            key,
            TextureEntry {
                group: Arc::clone(&group),
                used: true,
            },
        );
        group
    }

    /// End of a frame, after its submit and before what it let go of is
    /// destroyed: let go of the uniform bind groups over chunks
    /// `uniform_arena` no longer holds, and of the texture bind groups this
    /// frame did not use or whose texture `ledger` is about to destroy.
    pub(crate) fn end_frame(&mut self, uniform_arena: &GpuArena, ledger: &TextureLedger) {
        self.uniform
            .retain(|key, _| uniform_arena.holds(&key.chunk));
        // A texture hashes and compares by identity, which its interior
        // mutability does not touch.
        #[allow(clippy::mutable_key_type)]
        let retired: HashSet<&wgpu::Texture> = ledger.retired_textures().collect();
        let let_go = |view: &wgpu::TextureView| retired.contains(view.texture());
        self.texture.retain(|key, entry| {
            let keep =
                entry.used && !let_go(&key.view) && !key.second_view.as_ref().is_some_and(let_go);
            entry.used = false;
            keep
        });
    }

    pub(crate) fn counts(&self) -> BindGroupCounts {
        BindGroupCounts {
            cached: (self.uniform.len() + self.texture.len()) as u64,
            ..self.counts
        }
    }
}

impl crate::WgpuGfxCtx {
    /// How many bind groups this context has made, by kind, and how many it
    /// keeps for later frames.
    pub fn bind_group_counts(&self) -> BindGroupCounts {
        self.bind_groups.counts()
    }
}
