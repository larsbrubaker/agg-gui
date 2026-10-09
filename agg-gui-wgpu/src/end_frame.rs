//! `end_frame` implementation — flushes all deferred [`DrawCommand`]s into a
//! single wgpu command submission.
//!
//! Two-phase approach to satisfy wgpu's borrow rules:
//!
//! 1. **Prepare** — walk `commands`, allocate GPU buffers, build bind groups.
//!    All owned resources are collected in a `Vec<Prepared>`.  A *size stack*
//!    is simulated so each command's uniforms get the resolution of whichever
//!    render target is current at that point in the command list.
//! 2. **Execute** — open a `RenderPass` per render target, walk the `Prepared`
//!    list, and issue draw calls.  PushLayer/PopLayer end the current pass and
//!    start a new one on the layer texture or parent target.
//!
//! Multi-pass orchestration: each layer push/pop boundary is a render-pass
//! boundary in wgpu (a `RenderPass<'enc>` exclusively borrows its encoder, so
//! switching attachments requires ending and re-beginning the pass).

use std::sync::Arc;

use crate::end_frame_prepare::prepare_all;
use crate::pipelines::WgpuPipelines;
use crate::{LastEndFrameStats, WgpuGfxCtx};

/// One `(buffer, offset, size)` allocation from the per-frame arena pool.
/// Stored inside [`Prepared`] variants and read by `execute_one` to produce
/// a `wgpu::BufferSlice` for vertex / index binding, or a
/// `wgpu::BufferBinding` for a uniform bind group.
///
/// Holding the `Arc<wgpu::Buffer>` here keeps the underlying chunk alive
/// even if the arena later advances to a different chunk in the same frame
/// (see `buffer_arena` module docs).
#[derive(Clone)]
pub(crate) struct PreparedSlice {
    pub buf: Arc<wgpu::Buffer>,
    pub offset: u64,
    pub size: u64,
}

impl PreparedSlice {
    /// Sub-slice of the underlying buffer covering exactly `size` bytes
    /// starting at `offset`.  Used for `set_vertex_buffer` / `set_index_buffer`.
    #[inline]
    pub fn wgpu_slice(&self) -> wgpu::BufferSlice<'_> {
        self.buf.slice(self.offset..self.offset + self.size)
    }

    /// Binding descriptor for a uniform bind group.  `size = None` would
    /// mean "rest of buffer" — we always want the exact uniform-struct size
    /// because the arena packs multiple uniforms back-to-back into one chunk.
    #[inline]
    pub fn uniform_binding(&self) -> wgpu::BufferBinding<'_> {
        wgpu::BufferBinding {
            buffer: &self.buf,
            offset: self.offset,
            size: std::num::NonZeroU64::new(self.size),
        }
    }
}

// ---------------------------------------------------------------------------
// Per-command prepared GPU resources
// ---------------------------------------------------------------------------

pub(crate) enum Prepared {
    /// Pass-level clear — handled via `LoadOp::Clear` on the next pass open.
    Clear(wgpu::Color),
    /// Solid colour (no AA).
    Solid {
        vb: PreparedSlice,
        ib: PreparedSlice,
        index_count: u32,
        bg0: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
    },
    /// AA solid (per-vertex alpha from tess2 halo strips).
    AaSolid {
        vb: PreparedSlice,
        ib: PreparedSlice,
        index_count: u32,
        bg0: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
    },
    /// AA via texture lookup — agg-sharp `Graphics2DGpu` port.
    /// `bg1` is the cached alpha-step texture binding owned by
    /// `WgpuGfxCtx::aa_step_bg1`.
    AaTexture {
        vb: PreparedSlice,
        ib: PreparedSlice,
        index_count: u32,
        bg0: wgpu::BindGroup,
        bg1: Arc<wgpu::BindGroup>,
        clip: Option<[i32; 4]>,
    },
    /// Linear or radial gradient.
    Gradient {
        _ramp_tex: wgpu::Texture,
        _ramp_view: wgpu::TextureView,
        vb: PreparedSlice,
        ib: PreparedSlice,
        index_count: u32,
        bg0: wgpu::BindGroup,
        bg1: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
    },
    /// Textured quad (image blit).
    Textured {
        _texture: Arc<wgpu::Texture>,
        _view: wgpu::TextureView,
        vb: PreparedSlice,
        bg0: wgpu::BindGroup,
        bg1: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
    },
    /// LCD subpixel mask (3-pass, or single grayscale pass when `flatten`).
    LcdMask {
        _texture: Arc<wgpu::Texture>,
        _view: wgpu::TextureView,
        vb: PreparedSlice,
        ib: PreparedSlice,
        bg0s: [wgpu::BindGroup; 3],
        bg1: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
        flatten: bool,
    },
    /// LCD backbuffer (3-pass two-plane, or single flatten pass when `flatten`).
    LcbMask {
        _color_tex: Arc<wgpu::Texture>,
        _color_view: wgpu::TextureView,
        _alpha_tex: Arc<wgpu::Texture>,
        _alpha_view: wgpu::TextureView,
        vb: PreparedSlice,
        ib: PreparedSlice,
        bg0s: [wgpu::BindGroup; 3],
        bg1: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
        flatten: bool,
    },
    /// Begin rendering into a new layer texture.
    PushLayer {
        _texture: Arc<wgpu::Texture>,
        view: wgpu::TextureView,
        size: (u32, u32),
    },
    /// End layer rendering and composite onto the parent target.
    PopLayer {
        _texture: Arc<wgpu::Texture>,
        _view: wgpu::TextureView,
        vb: PreparedSlice,
        bg0: wgpu::BindGroup,
        bg1: wgpu::BindGroup,
        parent_clip: Option<[i32; 4]>,
    },
    /// End clip-layer rendering and composite onto the parent through the
    /// tessellated clip path (see `crate::layer_mask`).
    PopLayerMasked {
        _texture: Arc<wgpu::Texture>,
        _view: wgpu::TextureView,
        vb: PreparedSlice,
        ib: PreparedSlice,
        /// 0 for an empty clip — nothing is drawn.
        index_count: u32,
        bg0: wgpu::BindGroup,
        bg1: wgpu::BindGroup,
        parent_clip: Option<[i32; 4]>,
    },
    /// Composite a retained layer onto the current target — no layer-stack
    /// change.
    CompositeLayer {
        _texture: Arc<wgpu::Texture>,
        _view: wgpu::TextureView,
        vb: PreparedSlice,
        bg0: wgpu::BindGroup,
        bg1: wgpu::BindGroup,
        parent_clip: Option<[i32; 4]>,
    },
    /// Begin a blend-mode draw's coverage layer (see `crate::comp_op`).
    CompOpBegin {
        _texture: Arc<wgpu::Texture>,
        view: wgpu::TextureView,
        size: (u32, u32),
    },
    /// Composite a blend-mode draw onto the parent: copy the parent into
    /// `dest` within `clip`, then run the composite pass reading it.
    CompOpEnd {
        _layer: Arc<wgpu::Texture>,
        dest: wgpu::Texture,
        bg: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
    },
    /// Generic custom render hook (see `crate::custom_render`).  Treated as a
    /// pass break (current pass ends, the renderer records its own pass on the
    /// same encoder, parent pass reopens with `LoadOp::Load`) so a custom
    /// renderer targets the active layer when its widget is hosted in a window.
    Custom {
        renderer: crate::custom_render::SharedCustomRenderer,
        screen_rect: agg_gui::Rect,
        parent_clip: Option<[i32; 4]>,
    },
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

impl WgpuGfxCtx {
    pub(crate) fn flush_to_surface(&mut self, surface_view: &wgpu::TextureView) {
        let commands = std::mem::take(&mut self.commands);
        let command_count = commands.len() as u32;

        // Rewind the per-frame buffer pool to chunk 0 / offset 0. Last
        // frame's `Prepared` Vec was dropped at the end of the previous
        // `flush_to_surface`, so nothing external still references the
        // chunks; the wgpu::Buffers themselves are kept and overwritten via
        // one `queue.write_buffer` per chunk (`FrameArenas::flush` below)
        // instead of being reallocated.
        self.frame_arenas.begin_frame();

        // Wall-clock split of the three phases. We deliberately keep this
        // measurement in-renderer (vs. having the shell measure end_frame as
        // a whole) so consumers see prepare-vs-execute-vs-submit separately —
        // those three have very different optimisation strategies (CPU-side
        // buffer allocation, render-pass batching, driver/GPU sync).
        let t_prepare = web_time::Instant::now();
        let prepared = prepare_all(
            &self.device,
            &self.queue,
            &self.pipelines,
            &mut self.frame_arenas,
            &commands,
            self.viewport,
            &self.aa_step_bg1,
            &self.comp_op,
            self.surface_format,
        );
        // Upload everything `prepare_all` staged in the arenas — one write
        // per chunk.  Must precede the `queue.submit` below; timed as part of
        // prepare, where the per-allocation writes used to be.
        self.frame_arenas.flush(&self.queue);
        let prepare_us = t_prepare.elapsed().as_micros().min(u32::MAX as u128) as u32;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        // A blend-mode composite reads its target back.  Where the surface
        // can't be copied, the frame renders into a proxy that can, blitted
        // onto the surface at the end (see `crate::comp_op`).
        let reads_target = prepared
            .iter()
            .any(|p| matches!(p, Prepared::CompOpEnd { .. }));
        let readable_root = self
            .surface_texture
            .as_ref()
            .filter(|t| crate::comp_op::root_is_readable(t, self.viewport, self.surface_format));
        let proxy = (reads_target && readable_root.is_none())
            .then(|| crate::comp_op::alloc_proxy(&self.device, self.viewport, self.surface_format));
        let root = match &proxy {
            Some((texture, view)) => (view, Some(texture)),
            None => (surface_view, readable_root),
        };

        let t_execute = web_time::Instant::now();
        execute_prepared(
            &self.device,
            &self.queue,
            self.surface_format,
            &mut encoder,
            root,
            &self.pipelines,
            &self.comp_op,
            &prepared,
            self.viewport,
        );
        if let Some((_, view)) = &proxy {
            crate::comp_op::blit_proxy(
                &self.device,
                &mut encoder,
                &self.comp_op,
                view,
                surface_view,
            );
        }
        let execute_us = t_execute.elapsed().as_micros().min(u32::MAX as u128) as u32;

        let t_submit = web_time::Instant::now();
        self.queue.submit(std::iter::once(encoder.finish()));
        let submit_us = t_submit.elapsed().as_micros().min(u32::MAX as u128) as u32;

        self.last_end_frame_stats = LastEndFrameStats {
            prepare_us,
            execute_us,
            submit_us,
            command_count,
        };
    }
}
// ---------------------------------------------------------------------------
// Phase 2 — execute in render passes
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn execute_prepared<'a>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    surface_format: wgpu::TextureFormat,
    encoder: &mut wgpu::CommandEncoder,
    root: (&'a wgpu::TextureView, Option<&'a wgpu::Texture>),
    pipelines: &WgpuPipelines,
    comp_op: &crate::comp_op::CompOpGpu,
    prepared: &'a [Prepared],
    surface_viewport: (f32, f32),
) {
    // Initial clear: only honoured if the very first command is Clear.  Mid-frame
    // clears (after a draw) are skipped — the layer system makes them rare.
    let init_clear = match prepared.first() {
        Some(Prepared::Clear(c)) => Some(*c),
        _ => None,
    };

    // Stack of `(target_view, viewport_size, texture)`.  Borrowed from `root`
    // or `Prepared::PushLayer.view` for active layers.  The texture is what a
    // blend-mode composite copies its destination from.
    type Target<'t> = (&'t wgpu::TextureView, (f32, f32), Option<&'t wgpu::Texture>);
    let mut target_stack: Vec<Target<'a>> = vec![(root.0, surface_viewport, root.1)];

    let mut load_op: wgpu::LoadOp<wgpu::Color> = match init_clear {
        Some(c) => wgpu::LoadOp::Clear(c),
        None => wgpu::LoadOp::Load,
    };

    // After a PopLayer we must emit a composite quad at the start of the parent's
    // resumed pass — captured here between the closed layer pass and the reopened
    // parent pass.  The references point into `prepared`.
    // `indexed` is `Some((index_buffer, index_count))` for a masked clip-layer
    // composite (a mesh through the clip path) and `None` for the plain quad.
    type PendingComposite<'p> = (
        &'p PreparedSlice,
        Option<(&'p PreparedSlice, u32)>,
        &'p wgpu::BindGroup,
        &'p wgpu::BindGroup,
        Option<[i32; 4]>,
    );
    let mut pending_composite: Option<PendingComposite<'a>> = None;
    // A blend-mode composite, run first in the parent's resumed pass.
    type PendingCompOp<'p> = (&'p wgpu::BindGroup, (u32, u32, u32, u32));
    let mut pending_comp_op: Option<PendingCompOp<'a>> = None;

    let mut i = 0usize;

    // Each iteration of the outer loop runs exactly one render pass.  The inner
    // block scopes the pass so the encoder borrow ends when we exit it.
    while i < prepared.len() || pending_composite.is_some() || pending_comp_op.is_some() {
        let &(target_view, target_vp, _) = target_stack.last().unwrap();

        {
            let mut pass = begin_pass(encoder, target_view, load_op);
            pass.set_viewport(0.0, 0.0, target_vp.0, target_vp.1, 0.0, 1.0);
            if let Some((bg, rect)) = pending_comp_op.take() {
                crate::comp_op::draw_composite(&mut pass, comp_op, bg, rect);
            }

            // First, if a PopLayer is pending, emit its composite quad at the
            // start of this resumed parent pass — clipped to the scissor that
            // was active in the parent when the layer was pushed, so the blit
            // can't spill outside the parent's clip (e.g. over a title bar).
            if let Some((vb, indexed, bg0, bg1, parent_clip)) = pending_composite.take() {
                if apply_clip(&mut pass, parent_clip, target_vp) {
                    match indexed {
                        None => {
                            pass.set_pipeline(&pipelines.layer_pipeline);
                            pass.set_bind_group(0, bg0, &[]);
                            pass.set_bind_group(1, bg1, &[]);
                            pass.set_vertex_buffer(0, vb.wgpu_slice());
                            pass.draw(0..6, 0..1);
                        }
                        Some((ib, index_count)) if index_count > 0 => {
                            pass.set_pipeline(&pipelines.layer_mesh_pipeline);
                            pass.set_bind_group(0, bg0, &[]);
                            pass.set_bind_group(1, bg1, &[]);
                            pass.set_vertex_buffer(0, vb.wgpu_slice());
                            pass.set_index_buffer(ib.wgpu_slice(), wgpu::IndexFormat::Uint32);
                            pass.draw_indexed(0..index_count, 0, 0..1);
                        }
                        // Empty clip — nothing to draw.
                        Some(_) => {}
                    }
                }
            }

            // Drive the pass forward until end-of-list or a pass break.  Layer
            // push/pop and Custom all force the active 2-D pass to end so the
            // boundary handler below can do its work on the bare encoder.
            while i < prepared.len() {
                match &prepared[i] {
                    Prepared::PushLayer { .. }
                    | Prepared::PopLayer { .. }
                    | Prepared::PopLayerMasked { .. }
                    | Prepared::CompOpBegin { .. }
                    | Prepared::CompOpEnd { .. }
                    | Prepared::Custom { .. } => break,
                    other => {
                        execute_one(&mut pass, pipelines, other, target_vp);
                        i += 1;
                    }
                }
            }
            // pass is dropped here, releasing the encoder borrow.
        }

        // Subsequent passes use Load by default.
        load_op = wgpu::LoadOp::Load;

        // Process the boundary command (if any) to set up the next pass's state.
        if i < prepared.len() {
            match &prepared[i] {
                Prepared::PushLayer {
                    _texture: texture,
                    view,
                    size,
                }
                | Prepared::CompOpBegin {
                    _texture: texture,
                    view,
                    size,
                } => {
                    let size = (size.0 as f32, size.1 as f32);
                    target_stack.push((view, size, Some(&**texture)));
                    load_op = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
                    i += 1;
                }
                Prepared::CompOpEnd { dest, bg, clip, .. } => {
                    target_stack.pop();
                    let &(_, parent_vp, parent_texture) = target_stack.last().unwrap();
                    let rect = crate::comp_op::device_rect(*clip, parent_vp);
                    if let (Some(rect), Some(texture)) = (rect, parent_texture) {
                        crate::comp_op::copy_destination(encoder, texture, dest, rect);
                        pending_comp_op = Some((bg, rect));
                    }
                    i += 1;
                }
                Prepared::PopLayer {
                    vb,
                    bg0,
                    bg1,
                    parent_clip,
                    ..
                } => {
                    target_stack.pop();
                    pending_composite = Some((vb, None, bg0, bg1, *parent_clip));
                    i += 1;
                }
                Prepared::PopLayerMasked {
                    vb,
                    ib,
                    index_count,
                    bg0,
                    bg1,
                    parent_clip,
                    ..
                } => {
                    target_stack.pop();
                    pending_composite =
                        Some((vb, Some((ib, *index_count)), bg0, bg1, *parent_clip));
                    i += 1;
                }
                Prepared::Custom {
                    renderer,
                    screen_rect,
                    parent_clip,
                } => {
                    // Generic external render hook — see `custom_render` mod.
                    // Renders onto whatever target is current: the surface when
                    // the widget is at top level, the active window's layer view
                    // when hosted in a window.  No stack change; the next
                    // iteration reopens the same target with Load.
                    let target_size = (target_vp.0 as u32, target_vp.1 as u32);
                    let ctx = crate::custom_render::WgpuCustomRenderCtx {
                        device,
                        queue,
                        encoder,
                        target_view,
                        target_size,
                        surface_format,
                        screen_rect: *screen_rect,
                        parent_clip: *parent_clip,
                        pipelines,
                    };
                    renderer.borrow_mut().render(ctx);
                    i += 1;
                }
                _ => unreachable!("loop only breaks on pass-boundary commands"),
            }
        }
    }
}

/// Issue draw calls for a single non-layer-boundary prepared command into an
/// open render pass.
fn execute_one(
    pass: &mut wgpu::RenderPass,
    pipelines: &WgpuPipelines,
    item: &Prepared,
    vp: (f32, f32),
) {
    match item {
        Prepared::Clear(_) => {
            // LoadOp::Clear was used at pass open; mid-frame Clears ignored.
        }
        Prepared::Solid {
            vb,
            ib,
            index_count,
            bg0,
            clip,
        } => {
            if !apply_clip(pass, *clip, vp) {
                return;
            }
            pass.set_pipeline(&pipelines.solid_pipeline);
            pass.set_bind_group(0, bg0, &[]);
            pass.set_vertex_buffer(0, vb.wgpu_slice());
            pass.set_index_buffer(ib.wgpu_slice(), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*index_count, 0, 0..1);
        }
        Prepared::AaSolid {
            vb,
            ib,
            index_count,
            bg0,
            clip,
        } => {
            if !apply_clip(pass, *clip, vp) {
                return;
            }
            pass.set_pipeline(&pipelines.aa_solid_pipeline);
            pass.set_bind_group(0, bg0, &[]);
            pass.set_vertex_buffer(0, vb.wgpu_slice());
            pass.set_index_buffer(ib.wgpu_slice(), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*index_count, 0, 0..1);
        }
        Prepared::AaTexture {
            vb,
            ib,
            index_count,
            bg0,
            bg1,
            clip,
        } => {
            if !apply_clip(pass, *clip, vp) {
                return;
            }
            pass.set_pipeline(&pipelines.aa_texture_pipeline);
            pass.set_bind_group(0, bg0, &[]);
            pass.set_bind_group(1, &**bg1, &[]);
            pass.set_vertex_buffer(0, vb.wgpu_slice());
            pass.set_index_buffer(ib.wgpu_slice(), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*index_count, 0, 0..1);
        }
        Prepared::Gradient {
            vb,
            ib,
            index_count,
            bg0,
            bg1,
            clip,
            ..
        } => {
            if !apply_clip(pass, *clip, vp) {
                return;
            }
            pass.set_pipeline(&pipelines.gradient_pipeline);
            pass.set_bind_group(0, bg0, &[]);
            pass.set_bind_group(1, bg1, &[]);
            pass.set_vertex_buffer(0, vb.wgpu_slice());
            pass.set_index_buffer(ib.wgpu_slice(), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*index_count, 0, 0..1);
        }
        Prepared::Textured {
            vb, bg0, bg1, clip, ..
        } => {
            if !apply_clip(pass, *clip, vp) {
                return;
            }
            pass.set_pipeline(&pipelines.tex_pipeline);
            pass.set_bind_group(0, bg0, &[]);
            pass.set_bind_group(1, bg1, &[]);
            pass.set_vertex_buffer(0, vb.wgpu_slice());
            pass.draw(0..6, 0..1);
        }
        Prepared::LcdMask {
            vb,
            ib,
            bg0s,
            bg1,
            clip,
            flatten,
            ..
        } => {
            if !apply_clip(pass, *clip, vp) {
                return;
            }
            pass.set_bind_group(1, bg1, &[]);
            pass.set_vertex_buffer(0, vb.wgpu_slice());
            pass.set_index_buffer(ib.wgpu_slice(), wgpu::IndexFormat::Uint32);
            if *flatten {
                // Single alpha-writing grayscale pass (inside a layer). Uniforms
                // in bg0s[0] carry the colour; the channel field is ignored.
                pass.set_pipeline(&pipelines.text_gray);
                pass.set_bind_group(0, &bg0s[0], &[]);
                pass.draw_indexed(0..6, 0, 0..1);
            } else {
                let lcd_pipelines = [&pipelines.lcd_r, &pipelines.lcd_g, &pipelines.lcd_b];
                for ch in 0..3 {
                    pass.set_pipeline(lcd_pipelines[ch]);
                    pass.set_bind_group(0, &bg0s[ch], &[]);
                    pass.draw_indexed(0..6, 0, 0..1);
                }
            }
        }
        Prepared::LcbMask {
            vb,
            ib,
            bg0s,
            bg1,
            clip,
            flatten,
            ..
        } => {
            if !apply_clip(pass, *clip, vp) {
                return;
            }
            pass.set_bind_group(1, bg1, &[]);
            pass.set_vertex_buffer(0, vb.wgpu_slice());
            pass.set_index_buffer(ib.wgpu_slice(), wgpu::IndexFormat::Uint32);
            if *flatten {
                // Single flatten pass (inside a layer). Any of the three bind
                // groups works — they share resolution + global_alpha and only
                // differ in the (here-ignored) channel selector.
                pass.set_pipeline(&pipelines.lcb_flatten);
                pass.set_bind_group(0, &bg0s[0], &[]);
                pass.draw_indexed(0..6, 0, 0..1);
            } else {
                let lcb_pipelines = [&pipelines.lcb_r, &pipelines.lcb_g, &pipelines.lcb_b];
                for ch in 0..3 {
                    pass.set_pipeline(lcb_pipelines[ch]);
                    pass.set_bind_group(0, &bg0s[ch], &[]);
                    pass.draw_indexed(0..6, 0, 0..1);
                }
            }
        }
        Prepared::CompositeLayer {
            vb,
            bg0,
            bg1,
            parent_clip,
            ..
        } => {
            // Composite a retained layer onto the current target — no stack
            // change.  Clip to the scissor active when the composite was
            // requested so the blit can't paint outside the parent's clip.
            if !apply_clip(pass, *parent_clip, vp) {
                return;
            }
            pass.set_pipeline(&pipelines.layer_pipeline);
            pass.set_bind_group(0, bg0, &[]);
            pass.set_bind_group(1, bg1, &[]);
            pass.set_vertex_buffer(0, vb.wgpu_slice());
            pass.draw(0..6, 0..1);
        }
        // Pass-boundary commands are handled in the outer driver, not here.
        Prepared::PushLayer { .. }
        | Prepared::PopLayer { .. }
        | Prepared::PopLayerMasked { .. }
        | Prepared::CompOpBegin { .. }
        | Prepared::CompOpEnd { .. }
        | Prepared::Custom { .. } => {}
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn begin_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &'a wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// Apply `clip` (Y-up scissor stored in the draw-state stack) to the open
/// render pass and return whether the draw can proceed.
///
/// Returns `false` when the clip would reject every fragment (zero width or
/// zero height after intersection with the viewport).  Callers MUST skip
/// the subsequent `draw` / `draw_indexed` when this returns `false`,
/// because wgpu's render-pass scissor state is sticky — if we silently
/// don't update the scissor on a zero-area clip, the previous draw's
/// scissor leaks into this one and the draw paints unclipped.  The
/// canonical reproducer is a collapsed [`Window`]: its `clip_children_rect`
/// returns `(0, 0, w, 0)` (zero content height), and without this signal
/// the body widgets render on top of the title bar.
fn apply_clip(pass: &mut wgpu::RenderPass, clip: Option<[i32; 4]>, vp: (f32, f32)) -> bool {
    let vp_w = vp.0 as u32;
    let vp_h = vp.1 as u32;
    if let Some(scissor) = clip {
        let (x, y, w, h) = WgpuGfxCtx::yup_to_ydown_scissor(scissor, vp_h);
        let w = w.min(vp_w.saturating_sub(x));
        let h = h.min(vp_h.saturating_sub(y));
        if w > 0 && h > 0 {
            pass.set_scissor_rect(x, y, w, h);
            true
        } else {
            false
        }
    } else {
        pass.set_scissor_rect(0, 0, vp_w, vp_h);
        true
    }
}

#[cfg(test)]
#[path = "end_frame_tests.rs"]
mod tests;
