//! The bind groups an [`crate::SsaaFramebuffer`] blits with, made once and
//! kept on the framebuffer: one over its colour target (group 1, remade when
//! a resize replaces the target) and one over a small buffer of its blits'
//! uniforms (group 0, at a dynamic offset per blit).
//!
//! Why keep them: in Firefox a bind group is freed only when JavaScript's
//! garbage collector collects it, from a fixed per-device descriptor heap
//! (see `bind_group_cache.rs`), so a framebuffer blitted every frame (the
//! tumble cube, a shell's offscreen scene) must not make bind groups every
//! frame.
//!
//! The uniforms: each blit takes a slot in the buffer,
//! `min_uniform_buffer_offset_alignment` apart, written through the queue
//! (never through a mapping, so a blit on a lost device does not panic). Two
//! blits recorded into one submission must not share a slot, or the second's
//! write would reach the first too (the queue applies writes before the
//! submission's commands run); a later submission may reuse a slot, since
//! the queue applies its writes after the earlier submission's commands.
//!
//! `blit_downsample_in` runs in a custom render pass and knows its
//! submission: the context's submission number. (Neither `ensure_size`,
//! which the tumble cube calls before each of its draws, nor the window
//! frame number, the same over an overlay frame's submit, tells submissions
//! apart.) Its blits start at slot 0 in each new submission and take the
//! next slot after that; a submission with more blits than slots grows the
//! buffer (doubling, up to [`MAX_SLOTS`]), and the buffer it replaces goes to
//! the blit's `retire`, since the submission's earlier blits use it.
//!
//! The other blits (`blit_to`, `blit_downsample_to`, the 3x / 4x variants,
//! `blit_downsample_retiring`) cannot tell which submission they are in: a
//! caller may record several into one encoder, even inside a custom render
//! pass that calls `ensure_size` before each (demo-wgpu's bar grid). They
//! take the slots in turn round a buffer of [`MAX_SLOTS`], made at that size
//! by their first blit (growing it later would make bind groups in later
//! frames), so each keeps its own uniforms as long as one submission holds
//! fewer than [`MAX_SLOTS`] blits of the framebuffer.

use crate::pipelines::{TexUniforms, WgpuPipelines};
use crate::BindGroupCounts;

/// Uniform slots a framebuffer blitted with `blit_downsample_in` starts
/// with: room for a submission's blits of every caller in this workspace.
const FIRST_SLOTS: u64 = 4;
/// The most uniform slots a framebuffer has (64 KiB at 256-byte alignment):
/// what `blit_downsample_in` grows to, and what the other blits take in
/// turn. Two blits of one submission share uniforms only past this many
/// blits of the framebuffer in that submission.
pub(crate) const MAX_SLOTS: u64 = 256;

/// The buffer of blit uniforms and the bind group over it.
struct UniformSlots {
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
    slots: u64,
    /// Bytes from one slot to the next: the uniform struct rounded up to
    /// `min_uniform_buffer_offset_alignment`, as a dynamic offset must be.
    stride: u64,
}

impl UniformSlots {
    fn new(device: &wgpu::Device, pipelines: &WgpuPipelines, slots: u64) -> Self {
        let size = std::mem::size_of::<TexUniforms>() as u64;
        let align = u64::from(device.limits().min_uniform_buffer_offset_alignment).max(1);
        let stride = size.div_ceil(align) * align;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ssaa_blit_uniforms"),
            size: slots * stride,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ssaa_blit_bg0"),
            layout: &pipelines.tex_bgl0,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                // One slot's worth; the dynamic offset picks the slot.
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(size),
                }),
            }],
        });
        Self {
            buffer,
            group,
            slots,
            stride,
        }
    }
}

/// A framebuffer's blit bind groups. See the module docs.
#[derive(Default)]
pub(crate) struct BlitBinds {
    uniforms: Option<UniformSlots>,
    /// Over the colour target and the blit sampler; `None` until the first
    /// blit and after the target is replaced.
    texture: Option<wgpu::BindGroup>,
    /// The submission of the last blit when it was a `blit_downsample_in`
    /// one; `None` after any other blit.
    submission: Option<u64>,
    /// The slot the next blit takes (modulo the buffer's slots).
    next_slot: u64,
    counts: BindGroupCounts,
}

impl BlitBinds {
    /// A resize replaced the colour target: the next blit binds the new one.
    pub(crate) fn color_replaced(&mut self) {
        self.texture = None;
    }

    /// Group 1 over `view` (the colour target) and `sampler`.
    pub(crate) fn texture(
        &mut self,
        device: &wgpu::Device,
        pipelines: &WgpuPipelines,
        view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        if let Some(group) = &self.texture {
            return group.clone();
        }
        self.counts.texture += 1;
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ssaa_blit_bg1"),
            layout: &pipelines.tex_bgl1,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        self.texture = Some(group.clone());
        group
    }

    /// Write one blit's `uniforms` into a slot of its own and return group 0
    /// with that slot's dynamic offset. `submission` is the context's
    /// submission number for a `blit_downsample_in` blit and `None` for the
    /// others (see the module docs). An outgrown buffer goes to `retire`.
    pub(crate) fn uniforms(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipelines: &WgpuPipelines,
        submission: Option<u64>,
        uniforms: &TexUniforms,
        retire: &mut dyn FnMut(wgpu::Buffer),
    ) -> (wgpu::BindGroup, u32) {
        // Back to slot 0 only for a new submission after a blit of an earlier
        // one. After a blit of the other kind the slots run on: that blit
        // may be in this submission.
        if let (Some(now), Some(last)) = (submission, self.submission) {
            if now != last {
                self.next_slot = 0;
            }
        }
        self.submission = submission;
        let slot = self.next_slot;
        self.next_slot += 1;
        // `blit_downsample_in` needs a slot for each of the submission's
        // blits so far; the others take all of them in turn.
        let needed = if submission.is_some() {
            slot + 1
        } else {
            MAX_SLOTS
        };
        let slots = match self.uniforms.take() {
            Some(slots) if needed <= slots.slots || slots.slots >= MAX_SLOTS => slots,
            outgrown => {
                let count = match &outgrown {
                    None if submission.is_some() => FIRST_SLOTS,
                    None => MAX_SLOTS,
                    Some(old) => (old.slots * 2).max(needed).min(MAX_SLOTS),
                };
                if let Some(old) = outgrown {
                    retire(old.buffer);
                }
                self.counts.uniform += 1;
                UniformSlots::new(device, pipelines, count)
            }
        };
        let offset = (slot % slots.slots) * slots.stride;
        queue.write_buffer(&slots.buffer, offset, bytemuck::bytes_of(uniforms));
        let group = slots.group.clone();
        self.uniforms = Some(slots);
        // At most `MAX_SLOTS` slots of a few hundred bytes: far below 4 GiB.
        (group, offset as u32)
    }

    /// The uniform buffer, for the framebuffer to destroy when it drops.
    pub(crate) fn take_buffer(&mut self) -> Option<wgpu::Buffer> {
        self.uniforms.take().map(|slots| slots.buffer)
    }

    pub(crate) fn counts(&self) -> BindGroupCounts {
        BindGroupCounts {
            cached: u64::from(self.uniforms.is_some()) + u64::from(self.texture.is_some()),
            ..self.counts
        }
    }
}
