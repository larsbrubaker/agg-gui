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

/// Turn-key winit + wgpu shell for native platform shims.
#[cfg(not(target_arch = "wasm32"))]
pub mod native_shell;
#[cfg(not(target_arch = "wasm32"))]
pub use native_shell::NativeShellConfig;

/// Turn-key canvas + rAF + DOM-input shell for wasm platform shims.
#[cfg(target_arch = "wasm32")]
pub mod web_shell;

pub mod bar_grid;
mod bar_grid_math;
pub mod bar_grid_render;
pub use bar_grid::{BarGridWgpuRenderer, WgpuCubeWidget, CUBE_SCREEN_RECT};

pub mod custom_render;
pub use custom_render::{SharedCustomRenderer, WgpuCustomRender, WgpuCustomRenderCtx};

pub mod ssaa;
pub use ssaa::{ssaa_linear_scale, SsaaFramebuffer};

/// Screenshot read-back methods on [`WgpuGfxCtx`] (GPU→CPU frame copy).
mod screenshot_readback;

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

mod aa_step;
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
mod screenshot_capture;
/// Scaled / region capture readback — small-output companion to
/// `screenshot_capture`'s full-surface readbacks.
mod screenshot_scaled;
pub use screenshot_scaled::RectInPixels;
mod shaders;
mod text_render;

#[cfg(test)]
mod layer_text_readback_tests;
#[cfg(test)]
mod lcd_arc_cache_tests;
#[cfg(test)]
mod screenshot_scaled_tests;

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

/// One entry in the **LCD** Arc-keyed wgpu texture cache.
///
/// Distinct from [`ArcTextureEntry`] because the LCD cache is keyed on the
/// source `Vec`'s heap **buffer address** (`data.as_ref().as_ptr()`), not on
/// `Arc::as_ptr`.  The TextArea dirty-strip path mutates a retained LCD plane
/// *in place* via `Arc::make_mut`; when a `Weak` is outstanding that relocates
/// the Arc control block (so `Arc::as_ptr` changes every keystroke) while only
/// *moving* the `Vec` header — the byte buffer stays put.  Keying on the buffer
/// address therefore keeps the entry stable across in-place edits so the texture
/// allocation is reused instead of destroyed and rebuilt every keystroke.
///
/// `version` is the backbuffer's globally-unique `content_version` (from
/// `next_content_version()` in agg-gui, one atomic counter across all caches).
/// A freed buffer address can be recycled by an unrelated allocation, but the
/// version it carries will always differ from any this entry has seen, so a
/// stale entry can never falsely match — the mismatch forces a fresh write.
/// Dimensions are checked as an additional guard.  Content-addressed *immutable*
/// masks pass `version == 0` and are instead verified by Arc pointer identity
/// via the stored `weak` (those buffers never relocate).
pub(crate) struct LcdArcTextureEntry {
    pub(crate) weak: Weak<Vec<u8>>,
    pub(crate) texture: Arc<wgpu::Texture>,
    pub(crate) view: wgpu::TextureView,
    pub(crate) w: u32,
    pub(crate) h: u32,
    pub(crate) version: u64,
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
    /// Scissor active in the parent at push time — applied to the composite
    /// blit on pop so the layer can't paint outside the parent's clip.
    pub(crate) parent_clip: Option<[i32; 4]>,
    /// Set via `set_layer_opaque_backdrop` once the caller has covered this
    /// layer with opaque content; re-enables LCD subpixel text inside it.
    ///
    /// The default (`false`) is the safe one. A fresh layer texture is
    /// cleared to alpha 0 and the 3-pass subpixel composite writes only
    /// colour, so glyphs would keep alpha 0 and the pop composite would
    /// blend them additively toward white — the wash-out this flag's
    /// absence is designed to avoid. Once opaque content covers the
    /// region, destination alpha is already 1 where glyphs land and
    /// subpixel geometry is exactly as valid as against the backbuffer.
    pub(crate) opaque_backdrop: bool,
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
/// An in-flight, non-blocking readback of the capture texture for the **web**
/// screen-share sender.  A blocking `map_async` + `poll(Wait)` (as
/// [`DrawCtx::read_captured_screenshot`] does for native Save/Copy) deadlocks the
/// single-threaded browser: the map can only complete once control returns to the
/// JS event loop, but the blocking wait never yields.  So the sender starts a
/// readback one frame and harvests it a later frame via
/// [`WgpuGfxCtx::poll_capture_readback`].
pub(crate) struct PendingReadback {
    staging: wgpu::Buffer,
    w: u32,
    h: u32,
    padded_bpr: u32,
    done: std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

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
    /// Buffer-address-keyed cache for LCD coverage masks and backbuffer planes.
    pub(crate) lcd_arc_texture_cache: HashMap<usize, LcdArcTextureEntry>,

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

    /// In-flight async capture readback for the web screen-share sender.
    /// See [`PendingReadback`].
    pub(crate) pending_readback: Option<PendingReadback>,

    /// In-flight async *scaled / region* capture readback — a separate slot so
    /// periodic small thumbnails and a full-surface screenshot can be in
    /// flight at the same time.  See `screenshot_scaled`.
    pub(crate) pending_scaled_readback: Option<PendingReadback>,

    /// Pipeline + sampler + destination texture for the scaled capture blit.
    /// Built on first scaled capture; `None` until then.
    pub(crate) scaled_blit: Option<screenshot_scaled::ScaledBlit>,

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
        let aa_step_texture = aa_step::build_aa_step_texture(&device, &queue);
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
            pending_readback: None,
            pending_scaled_readback: None,
            scaled_blit: None,
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
    ///
    /// `flatten` routes to the single-pass, alpha-writing grayscale pipeline
    /// instead of the 3-pass subpixel one — set when the text lands inside a
    /// compositing layer, whose transparent destination the 3-pass path can't
    /// blend correctly (see `text_gray` in `pipelines.rs`).
    LcdMask {
        verts: [f32; 16],
        texture: Arc<wgpu::Texture>,
        view: wgpu::TextureView,
        color: Color,
        clip: Option<[i32; 4]>,
        flatten: bool,
    },
    /// LCD backbuffer (two-plane 3-pass blend).
    ///
    /// `flatten` (inside a layer) routes to the single-pass `lcb_flatten`
    /// pipeline.  `global_alpha` folds the context alpha into the premultiplied
    /// blit so cached LCD text fades under a direct `set_global_alpha`.
    LcbMask {
        verts: [f32; 16],
        color_tex: Arc<wgpu::Texture>,
        color_view: wgpu::TextureView,
        alpha_tex: Arc<wgpu::Texture>,
        alpha_view: wgpu::TextureView,
        clip: Option<[i32; 4]>,
        global_alpha: f32,
        flatten: bool,
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
        /// Scissor active in the PARENT at `push_layer` time.  The composite
        /// blit must honor it — otherwise a layer taller than its clipped
        /// content paints over sibling chrome (e.g. a window title bar).
        parent_clip: Option<[i32; 4]>,
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
        /// Scissor active on the target when the composite is requested.
        parent_clip: Option<[i32; 4]>,
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
