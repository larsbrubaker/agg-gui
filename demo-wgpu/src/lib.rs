//! `WgpuGfxCtx` — a hardware-accelerated [`DrawCtx`] implementation via `wgpu`.
//!
//! # Platform coverage
//!
//! | Target | Backend |
//! |---|---|
//! | Windows | Vulkan, DX12 |
//! | macOS / iOS | Metal |
//! | Linux / Android | Vulkan |
//! | WASM (`wasm32-unknown-unknown`) | WebGL2 (via `wgpu` `webgl` feature) |
//!
//! # Platform-split policy (mirrors `demo-gl`)
//!
//! This crate is the **shared wgpu backend + wgpu-using demo widgets**.
//! Platform shells (`demo-native`, `demo-wasm`) are pure OS shims; all
//! rendering code lives here so both targets execute identical compiled bytes.
//!
//! - Generic widget / layout code (no GPU dependency) → `demo-ui`
//! - wgpu-using demo widgets (bar grid, etc.) → here, in dedicated modules
//! - Platform shell (OS window/canvas, event loop, persistence) → `demo-native` / `demo-wasm`
//!
//! # Deferred draw command model
//!
//! Unlike the GL backend which submits draw calls immediately, `WgpuGfxCtx`
//! accumulates [`DrawCommand`] enums during `fill()` / `stroke()` / etc., then
//! flushes them all in [`WgpuGfxCtx::end_frame`] using a single
//! `wgpu::CommandEncoder`.  This avoids the render-pass borrow lifetime
//! conflict: a `RenderPass` exclusively borrows its encoder, preventing both
//! from living in the same struct simultaneously.
//!
//! # Coordinate system
//!
//! All incoming coordinates are **Y-up pixel space**: origin at the bottom-left
//! of the viewport, positive Y upward.  The vertex shader converts to NDC with
//! `ndc = (pos / resolution) * 2 - 1`.  Scissor rects are stored in Y-up form
//! and converted to wgpu's Y-down framebuffer convention inside `end_frame`.

pub mod frame;
pub use frame::{begin_frame, render_app_frame};

pub mod bar_grid;
mod bar_grid_math;
pub mod bar_grid_render;
pub use bar_grid::{BarGridWgpuRenderer, WgpuCubeWidget, CUBE_SCREEN_RECT};

pub mod custom_render;
pub use custom_render::{SharedCustomRenderer, WgpuCustomRender, WgpuCustomRenderCtx};

pub mod ssaa;
pub use ssaa::{ssaa_linear_scale, SsaaFramebuffer};

/// GPU handle passed to widgets via `DrawCtx::gl_paint` on the wgpu backend.
///
/// All fields are owned (cloned `Arc<...>` for device/queue, `wgpu::TextureView`
/// is internally ref-counted) so the struct is `'static` and works with the
/// `&dyn std::any::Any` plumbing of [`agg_gui::GlPaint`].
///
/// Painters create their own `wgpu::CommandEncoder` and submit it via
/// `queue.submit(...)`.  `WgpuGfxCtx` flushes any pending 2-D commands
/// before invoking the painter, so submissions interleave in the natural
/// paint order without an explicit barrier.
#[derive(Clone)]
pub struct WgpuPaintContext {
    /// Device used to build pipelines, buffers, and textures.
    pub device: Arc<wgpu::Device>,
    /// Queue used to submit the painter's command encoder.
    pub queue: Arc<wgpu::Queue>,
    /// Render-target view — same surface or layer texture the 2-D pipeline
    /// is rendering to this frame.  Painters open render passes against it
    /// with `LoadOp::Load` to overlay on existing content.
    pub target_view: wgpu::TextureView,
    /// Format of `target_view` — needed for pipeline `ColorTargetState`.
    pub surface_format: wgpu::TextureFormat,
    /// Full target dimensions in physical pixels.
    pub target_size: (u32, u32),
}

mod buffer_arena;
mod ctx_core;
mod draw_ctx_impl;
mod end_frame;
mod end_frame_prepare;
mod gradient;
mod image_blit;
mod layers;
pub mod pipelines;
mod primitives;
mod shaders;
mod text_render;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Weak};

use agg_gui::color::Color;
use agg_gui::draw_ctx::{FillRule, LinearGradientPaint, RadialGradientPaint};
use agg_gui::gl_renderer::GlyphCache;
use agg_gui::text::Font;
use agg_gui::TransAffine;
use agg_gui::{LineCap, LineJoin};
use agg_rust::path_storage::PathStorage;

use pipelines::WgpuPipelines;

// ---------------------------------------------------------------------------
// Arc-keyed texture cache entry
// ---------------------------------------------------------------------------

/// One entry in the Arc-keyed wgpu texture cache.  The `Weak` serves as a
/// liveness sentinel: when all strong refs to the source `Vec<u8>` are dropped
/// (typically because the widget's L1 pixel cache evicted the entry),
/// `weak.upgrade()` returns `None` and the entry is swept on the next access.
pub(crate) struct ArcTextureEntry {
    pub(crate) weak: Weak<Vec<u8>>,
    pub(crate) texture: Arc<wgpu::Texture>,
    pub(crate) view: wgpu::TextureView,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

// ---------------------------------------------------------------------------
// Saved draw state (for push_layer)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct SavedWgpuDrawState {
    pub(crate) viewport: (f32, f32),
    pub(crate) fill_color: Color,
    pub(crate) fill_linear_gradient: Option<LinearGradientPaint>,
    pub(crate) fill_radial_gradient: Option<RadialGradientPaint>,
    pub(crate) stroke_color: Color,
    pub(crate) stroke_linear_gradient: Option<LinearGradientPaint>,
    pub(crate) stroke_radial_gradient: Option<RadialGradientPaint>,
    pub(crate) line_width: f64,
    pub(crate) line_join: LineJoin,
    pub(crate) line_cap: LineCap,
    pub(crate) fill_rule: FillRule,
    pub(crate) miter_limit: f64,
    pub(crate) line_dash: Vec<f64>,
    pub(crate) dash_offset: f64,
    pub(crate) global_alpha: f64,
    pub(crate) state_stack: Vec<(TransAffine, Option<[i32; 4]>)>,
    pub(crate) font: Option<Arc<Font>>,
    pub(crate) font_size: f64,
    pub(crate) lcd_mode: bool,
}

// ---------------------------------------------------------------------------
// Layer types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub(crate) struct LayerRoundedClip {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
    pub(crate) r: f32,
}

/// One transient wgpu compositing layer.
pub(crate) struct WgpuLayerEntry {
    /// Render-attachment + sampler texture for this layer.
    pub(crate) texture: Arc<wgpu::Texture>,
    pub(crate) view: wgpu::TextureView,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) origin_x: f64,
    pub(crate) origin_y: f64,
    pub(crate) alpha: f64,
    pub(crate) saved: SavedWgpuDrawState,
    /// Non-None when this layer will be stored in `retained_layers` on pop.
    pub(crate) retained_key: Option<u64>,
    pub(crate) rounded_clip: Option<LayerRoundedClip>,
}

/// A retained layer that persists across frames (keyed by `u64` handle).
pub(crate) struct RetainedWgpuLayer {
    pub(crate) texture: Arc<wgpu::Texture>,
    pub(crate) view: wgpu::TextureView,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rounded_clip: Option<LayerRoundedClip>,
}

// ---------------------------------------------------------------------------
// WgpuGfxCtx
// ---------------------------------------------------------------------------

/// A [`DrawCtx`] that renders via `wgpu` (Vulkan / DX12 / Metal / WebGL2).
///
/// Create with [`WgpuGfxCtx::new`], passing a `wgpu::Device` and `wgpu::Queue`
/// that were obtained by the platform shell.  Each frame: call
/// [`render_app_frame`] (which calls [`reset`][WgpuGfxCtx::reset] and
/// `app.paint(ctx)`), then call [`end_frame`][WgpuGfxCtx::end_frame] with the
/// current surface texture view to flush all deferred draw commands.
pub struct WgpuGfxCtx {
    // ── wgpu core ────────────────────────────────────────────────────────────
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pub(crate) surface_format: wgpu::TextureFormat,
    pub(crate) viewport: (f32, f32),

    // ── render pipelines (created in Phase 2) ────────────────────────────────
    pub(crate) pipelines: WgpuPipelines,

    // ── deferred draw commands (flushed in end_frame — Phase 4) ──────────────
    pub(crate) commands: Vec<DrawCommand>,

    // ── texture caches (populated in Phase 6) ────────────────────────────────
    /// Generic slice-keyed cache: key is a FNV-like hash of (ptr, len, w, h, head/tail bytes).
    pub(crate) texture_cache: HashMap<u64, (Arc<wgpu::Texture>, wgpu::TextureView, u32, u32)>,
    pub(crate) texture_cache_order: VecDeque<u64>,
    /// Arc-pointer-keyed cache for `draw_image_rgba_arc` (Label backbuffers).
    pub(crate) arc_texture_cache: HashMap<usize, ArcTextureEntry>,
    /// Arc-pointer-keyed cache for LCD coverage masks.
    pub(crate) lcd_arc_texture_cache: HashMap<usize, ArcTextureEntry>,

    // ── layer stack (wired in Phase 8) ────────────────────────────────────────
    pub(crate) layer_stack: Vec<WgpuLayerEntry>,
    pub(crate) retained_layers: HashMap<u64, RetainedWgpuLayer>,

    // ── drawing state ────────────────────────────────────────────────────────
    pub(crate) fill_color: Color,
    pub(crate) fill_linear_gradient: Option<LinearGradientPaint>,
    pub(crate) fill_radial_gradient: Option<RadialGradientPaint>,
    pub(crate) stroke_color: Color,
    pub(crate) stroke_linear_gradient: Option<LinearGradientPaint>,
    pub(crate) stroke_radial_gradient: Option<RadialGradientPaint>,
    pub(crate) line_width: f64,
    pub(crate) line_join: LineJoin,
    pub(crate) line_cap: LineCap,
    pub(crate) fill_rule: FillRule,
    pub(crate) miter_limit: f64,
    pub(crate) line_dash: Vec<f64>,
    pub(crate) dash_offset: f64,
    pub(crate) global_alpha: f64,
    /// Each entry is `(transform, scissor_yup)` — scissor stored in Y-up screen
    /// coordinates; converted to Y-down at `end_frame` time.
    pub(crate) state_stack: Vec<(TransAffine, Option<[i32; 4]>)>,
    /// Path builder — stored in local Y-up coordinates.
    pub(crate) path: PathStorage,
    pub(crate) font: Option<Arc<Font>>,
    pub(crate) font_size: f64,
    pub(crate) lcd_mode: bool,

    /// Tessellated-glyph cache shared with the GL backend — produces XY
    /// triangles per `(font, glyph_id, size)` key.  Lives on the context so
    /// glyph tessellations persist across frames.
    pub(crate) glyph_cache: GlyphCache,

    /// Surface texture view for the current frame — set by [`begin_frame`],
    /// cleared by [`Self::end_frame`].  Required so widgets that issue raw GPU
    /// draws via `DrawCtx::gl_paint` can target the same attachment as the
    /// deferred 2-D pipeline without the platform shell having to plumb the
    /// view through every call.
    pub(crate) surface_view: Option<wgpu::TextureView>,
    /// Cloned handle to the current frame's surface texture (the underlying
    /// resource is internally ref-counted, so cloning the handle is cheap and
    /// keeps the texture alive past `frame.present()` only if we still hold a
    /// clone).  Used by [`Self::read_screenshot`] to issue a
    /// `copy_texture_to_buffer` after `end_frame` has flushed the render —
    /// the platform shell wires this up by calling `set_surface_texture`
    /// before paint.
    pub(crate) surface_texture: Option<wgpu::Texture>,
    /// Pixels captured during the active frame for the screenshot UI.  The
    /// platform shell must call [`Self::read_screenshot`] BEFORE
    /// `frame.present()` (the swap-chain owns the texture after present),
    /// stash the result here, then the screenshot orchestration picks it
    /// up via [`Self::take_pending_screenshot`] in its read-back closure.
    pub(crate) pending_screenshot: Option<(Vec<u8>, u32, u32)>,

    /// GPU-resident copy of the most recent surface contents — populated by
    /// [`DrawCtx::capture_screenshot`], sampled directly by
    /// [`DrawCtx::draw_captured_screenshot`].  Lives on the GPU so the
    /// screenshot preview pane can render it every frame with no CPU
    /// readback (the previous Vec<u8> + re-upload + mipmap gen path was
    /// blowing the frame budget under continuous capture).
    ///
    /// Pixels are pulled back to system memory only when the user clicks
    /// Save or Copy — see [`DrawCtx::read_captured_screenshot`].
    pub(crate) capture_texture: Option<(Arc<wgpu::Texture>, wgpu::TextureView, u32, u32)>,

    /// Per-frame chunked buffer pool — see [`buffer_arena`] module docs.
    /// All `DrawCommand`s in a single flush share these three buffers
    /// instead of allocating their own, which is the single biggest lever
    /// against the per-command `create_buffer_init` cost.
    pub(crate) frame_arenas: buffer_arena::FrameArenas,

    /// 1024×1 RGBA8 alpha-step texture for the AA-texture pipeline.
    /// Column 0 = `(255, 255, 255, 0)`, columns 1..1023 =
    /// `(255, 255, 255, 255)`.  Sampled LINEAR — produces the
    /// sub-texel-wide AA transition right on the polygon edge, exactly
    /// like agg-sharp's `aATextureImages[255]`.  See
    /// `agg_gui::gl_renderer::aa_texture_mesh` for the texcoord scheme
    /// that drives it.
    #[allow(dead_code)]
    pub(crate) aa_step_texture: Arc<wgpu::Texture>,
    #[allow(dead_code)]
    pub(crate) aa_step_view: wgpu::TextureView,
    pub(crate) aa_step_bg1: Arc<wgpu::BindGroup>,

    /// Per-phase wall-clock timings from the most recent `end_frame`. Populated
    /// inside `flush_to_surface` so platform shells (atomartist, marbles) can
    /// surface a true breakdown of where wgpu-side time goes without needing
    /// to fork the renderer for instrumentation. All numbers are wall-clock
    /// microseconds; `command_count` is the number of `DrawCommand`s walked.
    pub(crate) last_end_frame_stats: LastEndFrameStats,
}

/// Wall-clock breakdown of the most recent `WgpuGfxCtx::end_frame` call.
///
/// `prepare_us` is the per-command CPU walk that allocates wgpu buffers and
/// bind groups (often the dominant cost for command-heavy scenes).
/// `execute_us` is the render-pass walk that issues draw calls into the
/// command encoder. `submit_us` is the `queue.submit` cost — usually tiny on
/// native, occasionally large on WebGPU or when the driver is back-pressured.
#[derive(Debug, Default, Clone, Copy)]
pub struct LastEndFrameStats {
    pub prepare_us: u32,
    pub execute_us: u32,
    pub submit_us: u32,
    pub command_count: u32,
}

impl WgpuGfxCtx {
    /// Create a new `WgpuGfxCtx`.
    ///
    /// `device` and `queue` must come from a `wgpu::Adapter` whose surface
    /// is already configured with `surface_format`.  The caller retains
    /// ownership of the surface; this struct only receives `Arc` refs to
    /// the device and queue so both it and the platform shell can drive
    /// buffer writes and texture uploads on the same queue.
    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        width: f32,
        height: f32,
    ) -> Self {
        // 2-D pipelines stay sample_count=1: text and shapes already have
        // analytic edge AA via the tess2 halo strip + per-vertex alpha, so
        // hardware MSAA wouldn't add visible quality and would cost a full-
        // surface MSAA buffer (and per-layer ones) every frame.  MSAA belongs
        // scoped to the bar-grid renderer, which manages its own multi-sample
        // attachments and resolves into the active 1-sample target view.
        let pipelines = WgpuPipelines::new(&device, surface_format, 1);
        let frame_arenas = buffer_arena::FrameArenas::new(&device);

        // Build the 1024×1 RGBA8 alpha-step texture once and stash a
        // ready-to-bind bind group on the context — every AA-texture
        // draw can reuse this exact `bg1` without ever rebuilding it,
        // since the texture itself is immutable.
        let aa_step_texture = build_aa_step_texture(&device, &queue);
        let aa_step_view = aa_step_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let aa_step_bg1 = Arc::new(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aa_step_bg1"),
            layout: &pipelines.aa_texture_bgl1,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&aa_step_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&pipelines.linear_sampler),
                },
            ],
        }));
        let aa_step_texture = Arc::new(aa_step_texture);

        Self {
            device,
            queue,
            surface_format,
            viewport: (width, height),
            pipelines,
            commands: Vec::new(),
            texture_cache: HashMap::new(),
            texture_cache_order: VecDeque::new(),
            arc_texture_cache: HashMap::new(),
            lcd_arc_texture_cache: HashMap::new(),
            layer_stack: Vec::new(),
            retained_layers: HashMap::new(),
            fill_color: Color::rgba(0.0, 0.0, 0.0, 1.0),
            fill_linear_gradient: None,
            fill_radial_gradient: None,
            stroke_color: Color::rgba(0.0, 0.0, 0.0, 1.0),
            stroke_linear_gradient: None,
            stroke_radial_gradient: None,
            line_width: 1.0,
            line_join: LineJoin::Miter,
            line_cap: LineCap::Butt,
            fill_rule: FillRule::NonZero,
            miter_limit: 4.0,
            line_dash: Vec::new(),
            dash_offset: 0.0,
            global_alpha: 1.0,
            state_stack: vec![(TransAffine::new(), None)],
            path: PathStorage::new(),
            font: None,
            font_size: 16.0,
            lcd_mode: false,
            glyph_cache: GlyphCache::new(),
            surface_view: None,
            surface_texture: None,
            pending_screenshot: None,
            capture_texture: None,
            frame_arenas,
            aa_step_texture,
            aa_step_view,
            aa_step_bg1,
            last_end_frame_stats: LastEndFrameStats::default(),
        }
    }

    /// Wall-clock breakdown of the most recent `end_frame` flush. Returns
    /// zeroes until the first frame has been flushed. Designed for live perf
    /// HUDs in platform shells — see atomartist's `record_frame_timings` for
    /// an example consumer.
    pub fn last_end_frame_stats(&self) -> LastEndFrameStats {
        self.last_end_frame_stats
    }

    /// Reset drawing state for a new frame.  Preserves GPU resources.
    pub fn reset(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.fill_color = Color::rgba(0.0, 0.0, 0.0, 1.0);
        self.fill_linear_gradient = None;
        self.fill_radial_gradient = None;
        self.stroke_color = Color::rgba(0.0, 0.0, 0.0, 1.0);
        self.stroke_linear_gradient = None;
        self.stroke_radial_gradient = None;
        self.line_width = 1.0;
        self.fill_rule = FillRule::NonZero;
        self.miter_limit = 4.0;
        self.line_dash.clear();
        self.dash_offset = 0.0;
        self.global_alpha = 1.0;
        self.state_stack = vec![(TransAffine::new(), None)];
        self.path = PathStorage::new();
        self.font = None;
        self.font_size = 16.0;
        self.commands.clear();
        self.layer_stack.clear();
    }

    /// Enable / disable LCD subpixel text for this context.  Called each frame
    /// from `render_app_frame` with `font_settings::lcd_enabled()`.
    pub fn set_lcd_mode(&mut self, on: bool) {
        self.lcd_mode = on;
    }

    /// Flush all deferred draw commands into a single wgpu command submission.
    ///
    /// Must be called after `render_app_frame` and before `surface.present()`.
    /// The surface view used as the render target was stashed by
    /// [`begin_frame`][crate::begin_frame] — the platform shell does not need
    /// to pass it again here.
    pub fn end_frame(&mut self) {
        let Some(view) = self.surface_view.take() else {
            return;
        };
        self.flush_to_surface(&view);
    }

    /// Borrow the shared 2-D pipeline collection.  Exposed so platform
    /// shells (currently `demo-wasm`) can drive a [`SsaaFramebuffer::blit_to`]
    /// when they need to composite an intermediate scene texture onto the
    /// real swap-chain surface — see the comment on
    /// [`SsaaFramebuffer::resolve_texture`] for the WebGL2 scene-buffer
    /// pattern.  The returned `&WgpuPipelines` only exposes the fields
    /// `pub(crate)` library code already uses; external callers can pass
    /// it back into other library APIs but cannot reach in directly.
    pub fn pipelines(&self) -> &pipelines::WgpuPipelines {
        &self.pipelines
    }

    /// Borrow a clone-able handle to the wgpu device used for resource
    /// allocation.  Exposed alongside [`Self::pipelines`] so platform
    /// shells driving an external blit pass (currently `demo-wasm`'s
    /// scene-buffer → surface composite) don't need to hold a duplicate
    /// `Arc<wgpu::Device>` themselves.
    pub fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    /// Borrow a clone-able handle to the wgpu queue.  Same rationale as
    /// [`Self::device`] — the shell submits the scene-blit encoder
    /// through this queue.
    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.queue
    }

    /// Queue a custom wgpu render pass to run at the current point in the
    /// frame's draw order. The user-supplied [`WgpuCustomRender`] is invoked
    /// from `end_frame` after the active 2-D pass closes; subsequent 2-D
    /// content reopens with `LoadOp::Load` so it composites on top.
    ///
    /// `screen_rect` is the widget's logical-pixel rect in agg-gui Y-up
    /// coords. The implementor receives it via
    /// [`WgpuCustomRenderCtx::screen_rect`] and is responsible for any
    /// scissor / viewport conversions to wgpu's Y-down convention.
    pub fn push_custom_render(
        &mut self,
        renderer: custom_render::SharedCustomRenderer,
        screen_rect: agg_gui::Rect,
    ) {
        let parent_clip = self.current_clip();
        self.commands.push(DrawCommand::Custom {
            renderer,
            screen_rect,
            parent_clip,
        });
    }

    /// Stash a handle to the current frame's surface texture so a later
    /// [`Self::read_screenshot`] call can copy from it.  Called from the
    /// platform shell with `frame.texture.clone()` BEFORE [`begin_frame`].
    /// `wgpu::Texture` is internally ref-counted, so the clone is cheap.
    pub fn set_surface_texture(&mut self, tex: wgpu::Texture) {
        self.surface_texture = Some(tex);
    }

    /// Stash captured screenshot pixels for the read-back closure to pick
    /// up.  See [`Self::pending_screenshot`] / [`Self::take_pending_screenshot`].
    pub fn set_pending_screenshot(&mut self, captured: (Vec<u8>, u32, u32)) {
        self.pending_screenshot = Some(captured);
    }

    /// Consume the pending screenshot pixels — returns `(Vec::new(), 0, 0)`
    /// when none are stashed (typical non-capture frames).  Called by the
    /// `agg_gui::screenshot::run_frame_with_capture` read-back closure.
    pub fn take_pending_screenshot(&mut self) -> (Vec<u8>, u32, u32) {
        self.pending_screenshot.take().unwrap_or((Vec::new(), 0, 0))
    }

    /// Read the current frame's rendered pixels back to CPU memory as a
    /// top-down RGBA8 buffer.  Returns `(pixels, width, height)`.
    /// The first `width * 4` bytes are the TOP row (Y-down image order).
    ///
    /// Must be called AFTER [`Self::end_frame`] has submitted the render and
    /// BEFORE the platform shell calls `frame.present()`.  Requires the
    /// platform shell to have called [`Self::set_surface_texture`] earlier
    /// in the frame so we hold a handle into the surface that's still
    /// valid post-render.
    ///
    /// Returns an empty buffer if no surface texture is currently stashed.
    pub fn read_screenshot(&self) -> (Vec<u8>, u32, u32) {
        let Some(texture) = self.surface_texture.as_ref() else {
            return (Vec::new(), 0, 0);
        };
        let size = texture.size();
        let w = size.width;
        let h = size.height;
        if w == 0 || h == 0 {
            return (Vec::new(), 0, 0);
        }

        // wgpu requires `bytes_per_row` to be a multiple of
        // COPY_BYTES_PER_ROW_ALIGNMENT (256).  We allocate a padded buffer
        // for the copy and strip the padding row-by-row when assembling the
        // returned `Vec<u8>`.
        const ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let unpadded_bpr = w * 4;
        let padded_bpr = unpadded_bpr.div_ceil(ALIGN) * ALIGN;
        let buffer_size = (padded_bpr as u64) * (h as u64);

        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot_staging"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("screenshot_copy"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        // Map the staging buffer.  `map_async` is async via callback; we
        // poll the device until the map completes.  On native this is fine
        // (synchronous from the caller's POV); on WASM the wgpu webgl
        // backend resolves the future on the JS event-loop tick that the
        // surrounding render loop is running on, so this still works
        // because the JS harness drives `render()` from a microtask.
        let slice = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = sender.send(res);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let map_result = receiver
            .recv()
            .expect("map_async sender dropped before resolving");
        if map_result.is_err() {
            return (Vec::new(), 0, 0);
        }

        // Surface format may be Bgra8Unorm; PNG / JS expects RGBA so swap
        // R↔B per pixel as we copy.  Surface textures are Y-down, which
        // matches the screenshot module's "TOP row first" convention, so
        // no row flip needed.
        let bgra = matches!(
            self.surface_format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut out = Vec::with_capacity((w as usize) * (h as usize) * 4);
        {
            let view = slice.get_mapped_range();
            for row in 0..h as usize {
                let start = row * padded_bpr as usize;
                let end = start + unpadded_bpr as usize;
                let src = &view[start..end];
                if bgra {
                    for px in src.chunks_exact(4) {
                        out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                    }
                } else {
                    out.extend_from_slice(src);
                }
            }
        }
        staging.unmap();
        (out, w, h)
    }
}

// ---------------------------------------------------------------------------
// Deferred draw command list
// ---------------------------------------------------------------------------

/// One deferred draw call, accumulated during `fill()` / `stroke()` / etc.
/// and flushed in `end_frame()`.
///
/// Using an enum lets `end_frame` batch consecutive commands on the same
/// render target into a single `wgpu::RenderPass`, which avoids the
/// render-pass borrow lifetime conflict inherent to wgpu's API.
pub(crate) enum DrawCommand {
    /// Solid-color fill/stroke (no AA halo).
    Solid {
        verts: Vec<[f32; 2]>,
        indices: Vec<u32>,
        color: Color,
        global_alpha: f32,
        clip: Option<[i32; 4]>,
    },
    /// AA solid-color fill/stroke (per-vertex alpha from tess2 halo strips).
    AaSolid {
        verts: Vec<[f32; 3]>,
        indices: Vec<u32>,
        color: Color,
        global_alpha: f32,
        clip: Option<[i32; 4]>,
    },
    /// Texture-based AA solid fill/stroke — direct port of agg-sharp's
    /// `Graphics2DGpu` pipeline.  `verts` carries `(pos.xy, uv.xy)` from
    /// `agg_gui::gl_renderer::tessellate_path_aa_texture`; the fragment
    /// shader samples the 1024-wide alpha-step texture (`ctx.aa_step_view`)
    /// to recover the per-pixel coverage.
    AaTexture {
        verts: Vec<agg_gui::gl_renderer::AaTexVertex>,
        indices: Vec<u32>,
        color: Color,
        global_alpha: f32,
        clip: Option<[i32; 4]>,
    },
    /// Linear or radial gradient fill.
    Gradient {
        verts: Vec<[f32; 3]>,
        indices: Vec<u32>,
        uniforms: gradient::GradientUniforms,
        ramp: Vec<u8>,
        clip: Option<[i32; 4]>,
    },
    /// Textured quad (image blit).
    Textured {
        verts: [f32; 24],
        texture: Arc<wgpu::Texture>,
        view: wgpu::TextureView,
        nearest: bool,
        /// RGBA multiplier applied in the fragment shader. `[1, 1, 1, 1]`
        /// is a straight blit; `[1, 1, 1, a]` fades the image to alpha
        /// `a`. Snapshotted from the context's `global_alpha` at draw
        /// time so fades follow the standard `set_global_alpha` knob.
        tint: [f32; 4],
        clip: Option<[i32; 4]>,
    },
    /// LCD subpixel mask (3-pass write-mask blend).
    LcdMask {
        verts: [f32; 16],
        texture: Arc<wgpu::Texture>,
        view: wgpu::TextureView,
        color: Color,
        clip: Option<[i32; 4]>,
    },
    /// LCD backbuffer (two-plane 3-pass blend).
    LcbMask {
        verts: [f32; 16],
        color_tex: Arc<wgpu::Texture>,
        color_view: wgpu::TextureView,
        alpha_tex: Arc<wgpu::Texture>,
        alpha_view: wgpu::TextureView,
        clip: Option<[i32; 4]>,
    },
    /// Clear the current render target to a solid color.
    Clear(Color),
    /// Begin rendering into a new layer texture.
    PushLayer {
        texture: Arc<wgpu::Texture>,
        view: wgpu::TextureView,
        width: u32,
        height: u32,
    },
    /// Composite the topmost layer texture into its parent and resume the
    /// parent render target.
    PopLayer {
        texture: Arc<wgpu::Texture>,
        view: wgpu::TextureView,
        origin_x: f32,
        origin_y: f32,
        layer_w: u32,
        layer_h: u32,
        alpha: f32,
        rounded_clip: Option<LayerRoundedClip>,
    },
    /// Composite a previously-retained layer onto the current render target
    /// without entering it as a draw target.  Used by `composite_retained_layer`.
    CompositeLayer {
        texture: Arc<wgpu::Texture>,
        view: wgpu::TextureView,
        origin_x: f32,
        origin_y: f32,
        layer_w: u32,
        layer_h: u32,
        alpha: f32,
        rounded_clip: Option<LayerRoundedClip>,
    },
    /// Render the 3-D bar-grid scene into the current render target.  The
    /// renderer is shared with [`bar_grid::WgpuCubeWidget`] via `Rc<RefCell<>>`
    /// so it persists across frames; `execute_prepared` ends the active 2-D
    /// pass, drives the renderer onto the active layer or surface, then
    /// reopens the 2-D pass with `LoadOp::Load`.
    DrawBarGrid {
        renderer: std::rc::Rc<std::cell::RefCell<Option<bar_grid::BarGridWgpuRenderer>>>,
        screen_rect: agg_gui::Rect,
        parent_clip: Option<[i32; 4]>,
    },
    /// Generic custom-render hook — dispatches to user code implementing
    /// [`WgpuCustomRender`].  Same pass-break / reopen semantics as
    /// `DrawBarGrid`. Pushed via [`WgpuGfxCtx::push_custom_render`].
    Custom {
        renderer: custom_render::SharedCustomRenderer,
        screen_rect: agg_gui::Rect,
        parent_clip: Option<[i32; 4]>,
    },
}

/// Build the 1024×1 RGBA8 alpha-step texture used by the AA-texture
/// pipeline.  Direct port of `Graphics2DGpu::CheckLineImageCache` in
/// agg-sharp (we only need the α=255 variant since the shader
/// multiplies in the polygon's colour alpha as a uniform).
///
/// Pixel layout:
/// - Column 0:        `(255, 255, 255, 0)`  ← fully transparent
/// - Columns 1..1023: `(255, 255, 255, 255)` ← fully opaque
///
/// Sampled LINEAR, the boundary between texel 0 and texel 1 produces a
/// sub-texel α ramp — that's the AA edge.  See
/// `agg_gui::gl_renderer::aa_texture_mesh` for the texcoord scheme.
fn build_aa_step_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    const W: u32 = 1024;
    let mut pixels = vec![255u8; (W as usize) * 4];
    // Column 0: zero out the alpha byte (offset 3 in RGBA).
    pixels[3] = 0;

    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aa_step"),
        size: wgpu::Extent3d {
            width: W,
            height: 1,
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
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(W * 4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: W,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    tex
}
