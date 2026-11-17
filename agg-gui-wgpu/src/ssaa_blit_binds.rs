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
//! The uniforms: every blit of a frame takes a slot of its own in the
//! buffer, `min_uniform_buffer_offset_alignment` apart, written through the
//! queue (never through a mapping, so a blit on a lost device does not
//! panic). Two blits recorded into one submission never share a slot, or
//! the second's write would reach the first too (the queue applies writes
//! before the submission's commands run). A new frame starts again at slot
//! 0: the queue applies its writes after the previous submission's commands,
//! so they still read their own. A frame is told apart by the context's
//! submission number when the blit runs inside a custom render pass
//! (`blit_downsample_in`; the tumble cube calls `ensure_size` before each of
//! its draws, so that cannot mark a frame, and the window frame number stays
//! the same over an overlay frame's submit), and otherwise by
//! [`crate::SsaaFramebuffer::ensure_size`], which the framebuffer's
//! documentation asks for once a frame. A frame with more blits than slots
//! grows the buffer (doubling, up to [`MAX_SLOTS`]); the buffer it replaces
//! goes to the blit's `retire`, since the frame's earlier blits use it.

use crate::pipelines::{TexUniforms, WgpuPipelines};
use crate::BindGroupCounts;

/// Which frame a blit belongs to, as far as the framebuffer can tell.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlitFrame {
    /// A custom render context's submission number.
    Context(u64),
    /// How many times `ensure_size` had been called.
    Sized(u64),
}

/// Uniform slots a framebuffer starts with: room for a frame's blits of
/// every caller in this workspace.
const FIRST_SLOTS: u64 = 4;
/// The most uniform slots a framebuffer grows to (256 KiB at 256-byte
/// alignment). Past this many blits in one frame, or when a caller blits
/// outside a custom render pass without ever calling `ensure_size`, slots
/// are taken round again: only a frame with more blits than this could see
/// two of them share uniforms.
pub(crate) const MAX_SLOTS: u64 = 1024;

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
    /// The frame the last blit belonged to.
    frame: Option<BlitFrame>,
    /// The slot the next blit of `frame` takes.
    next_slot: u64,
    /// `ensure_size` calls so far.
    sized: u64,
    counts: BindGroupCounts,
}

impl BlitBinds {
    /// `ensure_size` ran: a new frame for blits outside a custom render pass,
    /// and, when `color_replaced`, a new colour target to bind.
    pub(crate) fn ensure_size(&mut self, color_replaced: bool) {
        self.sized += 1;
        if color_replaced {
            self.texture = None;
        }
    }

    /// The frame of a blit run in the custom render pass of submission
    /// `context_frame`, or outside one when `None`.
    pub(crate) fn frame(&self, context_frame: Option<u64>) -> BlitFrame {
        match context_frame {
            Some(frame) => BlitFrame::Context(frame),
            None => BlitFrame::Sized(self.sized),
        }
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

    /// Write one blit's `uniforms` of `frame` into a slot of its own and
    /// return group 0 with that slot's dynamic offset. A buffer outgrown by
    /// the frame goes to `retire`.
    pub(crate) fn uniforms(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipelines: &WgpuPipelines,
        frame: BlitFrame,
        uniforms: &TexUniforms,
        retire: &mut dyn FnMut(wgpu::Buffer),
    ) -> (wgpu::BindGroup, u32) {
        if self.frame != Some(frame) {
            self.frame = Some(frame);
            self.next_slot = 0;
        }
        let slot = self.next_slot;
        self.next_slot += 1;
        let slots = match self.uniforms.take() {
            Some(slots) if slot < slots.slots || slots.slots >= MAX_SLOTS => slots,
            outgrown => {
                let count = outgrown
                    .as_ref()
                    .map_or(FIRST_SLOTS, |old| (old.slots * 2).min(MAX_SLOTS));
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
