//! Public hook for user widgets to inject their own wgpu render pass(es)
//! into the agg-gui frame.
//!
//! # Why
//!
//! agg-gui paints 2-D widgets via deferred `DrawCommand`s that flush into a
//! single `wgpu::CommandEncoder` in [`WgpuGfxCtx::end_frame`](crate::WgpuGfxCtx::end_frame). A
//! 3-D viewport widget (e.g. AtomArtist's `Viewport3dWidget`) needs to run
//! its own render pass(es) interleaved with that 2-D stream so its output
//! ends up on the same surface or layer texture, with depth-correct
//! ordering relative to surrounding 2-D content.
//!
//! The demo's own `WgpuCubeWidget` used to push a hard-coded
//! `DrawCommand::DrawBarGrid` variant directly into the command list — which
//! worked but was not extensible, and it kept demo content inside the
//! renderer. This module generalises that pattern so any widget can plug in;
//! the cube now rides this path like everyone else.
//!
//! # How to use
//!
//! 1. Define a struct that implements [`WgpuCustomRender`].
//! 2. Hold it in `Rc<RefCell<dyn WgpuCustomRender>>` so the widget can
//!    keep a clone (lazy-init / persistent buffers across frames).
//! 3. From your widget's `paint(ctx)`, downcast `ctx` to
//!    [`WgpuGfxCtx`](crate::WgpuGfxCtx) via
//!    [`DrawCtx::as_any_mut`](agg_gui::draw_ctx::DrawCtx::as_any_mut) and call
//!    [`WgpuGfxCtx::push_custom_render`](crate::WgpuGfxCtx::push_custom_render).
//!
//! # Pass semantics
//!
//! When the executor reaches a `DrawCommand::Custom` entry, it:
//!   1. Ends the active 2-D render pass.
//!   2. Calls [`WgpuCustomRender::render`] with a [`WgpuCustomRenderCtx`]
//!      pointing at the same encoder + active target view.
//!   3. Reopens a 2-D pass with `LoadOp::Load` so subsequent 2-D content
//!      composites cleanly on top.
//!
//! The custom renderer may record any number of its own passes against
//! the encoder, including offscreen passes followed by a blit to
//! `target_view`. Conventionally the implementor should leave its target
//! state untouched after returning (no in-progress pass, no orphaned
//! buffer mappings).

use std::cell::RefCell;
use std::rc::Rc;

use crate::pipelines::WgpuPipelines;

/// Inputs handed to a [`WgpuCustomRender`] implementor at execution time.
///
/// Field invariants:
///   - `encoder` is borrowed for the duration of `render` only — record
///     into it but don't take ownership.
///   - `target_view` is the surface or active layer texture view; the 2-D
///     pass on either side of this call uses the same view.
///   - `target_size` matches the active target's full pixel dimensions.
///   - `screen_rect` is the widget's logical-pixel rect in **bottom-up
///     Y-up coords** (agg-gui convention). Convert to wgpu's top-down
///     Y-down convention as needed when computing scissor / viewport.
///   - `pipelines` is the shared 2-D pipeline collection.  Implementors
///     that own an [`crate::ssaa::SsaaFramebuffer`] can call
///     [`crate::ssaa::SsaaFramebuffer::blit_to`] with this to composite
///     their offscreen output onto `target_view` through the same
///     textured-quad pipeline the 2-D path uses.
///
/// Only the renderer constructs this (from `end_frame`); implementors receive
/// one and read it. It is `#[non_exhaustive]` so further per-pass state can be
/// added without a breaking release.
#[non_exhaustive]
pub struct WgpuCustomRenderCtx<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub target_view: &'a wgpu::TextureView,
    pub target_size: (u32, u32),
    pub surface_format: wgpu::TextureFormat,
    pub screen_rect: agg_gui::Rect,
    /// Active scissor inherited from the agg-gui clip stack (or `None` if
    /// the parent pass had no clip). Format is `[x, y, w, h]` in wgpu
    /// top-down pixel coords.
    pub parent_clip: Option<[i32; 4]>,
    /// Shared 2-D pipeline collection — exposed so an offscreen-buffered
    /// custom renderer can blit its resolved framebuffer onto the active
    /// 2-D render target without rebuilding the textured-quad pipeline
    /// itself.  See [`crate::ssaa::SsaaFramebuffer::blit_to`].
    pub pipelines: &'a WgpuPipelines,
    /// The adapter `device` came from, for what a device cannot report
    /// (per-format texture features, for one). `None` only when the
    /// context's creator never called
    /// [`WgpuGfxCtx::set_adapter`](crate::WgpuGfxCtx::set_adapter); the
    /// native and web shells and the headless frame always do.
    pub adapter: Option<&'a wgpu::Adapter>,
    /// The adapter's downlevel capabilities, so a pass can be chosen from
    /// what the adapter supports without test pipelines: WebGL2, for one,
    /// lacks `DownlevelFlags::INDEPENDENT_BLEND` (different blends per
    /// colour target in one pipeline). `None` exactly when
    /// [`Self::adapter`] is.
    pub downlevel: Option<&'a wgpu::DownlevelCapabilities>,
    /// How many frames this device has lost so far: frames whose GPU work
    /// wgpu rejected — a validation error from any pass in the frame (this
    /// one's or another widget's), or running out of GPU memory — as counted
    /// by the device's [`crate::UncapturedErrors`]. A frame's commands are
    /// submitted together, so a rejection throws away everything recorded
    /// into [`Self::encoder`] that frame.
    ///
    /// A pass that keeps its output between frames (reusing a finished
    /// picture while nothing in its view changed) stores this value when it
    /// records, and treats the kept output as never drawn when a later frame
    /// shows a different value. The error is reported when the frame is
    /// submitted, and in the browser only once the GPU process answers, so
    /// the value moves a frame or more after the frame that was lost: the
    /// next frames may still see the old value.
    ///
    /// It never decreases on one device. A device-loss rebuild starts a new
    /// device, whose count starts again at 0 (a pass has to rebuild its GPU
    /// resources then anyway), so compare for any change, not for an
    /// increase. Always 0 when the context's creator never called
    /// [`WgpuGfxCtx::set_uncaptured_errors`](crate::WgpuGfxCtx::set_uncaptured_errors);
    /// the native and web shells always do. The headless frame's device keeps
    /// wgpu's panicking handler unless a test installs one and records it.
    pub lost_frames: u64,
}

/// Trait implemented by widgets that want to inject custom wgpu render
/// commands into the frame.
///
/// `render` is called from `end_frame()` once per matching
/// `DrawCommand::Custom` queued during
/// `paint()`. Implementations are typically held in
/// `Rc<RefCell<dyn WgpuCustomRender>>` so the widget can lazy-init GPU
/// state on first use and persist it across frames.
pub trait WgpuCustomRender {
    fn render(&mut self, ctx: WgpuCustomRenderCtx<'_>);
}

/// Convenience type alias for the boxed-trait pattern most callers use.
pub type SharedCustomRenderer = Rc<RefCell<dyn WgpuCustomRender>>;
