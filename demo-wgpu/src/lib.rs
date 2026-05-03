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
pub use bar_grid::{BarGridWgpuRenderer, WgpuCubeWidget, CUBE_SCREEN_RECT};

mod ctx_core;
mod draw_ctx_impl;
mod end_frame;
mod gradient;
mod image_blit;
mod layers;
mod pipelines;
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
        let pipelines = WgpuPipelines::new(&device, surface_format);
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
        }
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

    /// Flush all deferred draw commands into a single wgpu command submission
    /// and present the surface.
    ///
    /// Must be called after `render_app_frame` and before `surface.present()`.
    pub fn end_frame(&mut self, surface_view: &wgpu::TextureView) {
        self.flush_to_surface(surface_view);
    }

    /// Read the current frame's rendered pixels back to CPU memory as a
    /// top-down RGBA8 buffer.  Returns `(pixels, width, height)`.
    /// The first `width * 4` bytes are the TOP row (Y-down image order).
    pub fn read_screenshot(&self) -> (Vec<u8>, u32, u32) {
        todo!("Phase 10: implement GPU readback via copy_texture_to_buffer")
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
    /// Inline GPU content drawn by a [`agg_gui::GlPaint`] implementor.
    GlPaint {
        screen_rect: agg_gui::Rect,
        /// Heap-boxed to keep DrawCommand object-safe.
        painter: Box<dyn agg_gui::GlPaint>,
    },
}
