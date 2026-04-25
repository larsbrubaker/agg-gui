//! `GlGfxCtx` — a hardware-accelerated [`DrawCtx`] implementation for
//! WebGL2 / OpenGL via `glow`.
//!
//! # Platform-split policy (kept identical across `demo-native`, `demo-wasm`, `demo-gl`)
//!
//! This crate is the **shared GL backend + GL-using demo widgets**.
//! It exists so the platform shells (`demo-native`, `demo-wasm`) can
//! be pure OS shims while everything that touches `glow` — the
//! `DrawCtx` impl, the per-frame helpers, AND any demo widget that
//! needs raw GL (e.g. the 3D Animation `GlCubeWidget` in
//! [`bar_grid`]) — has exactly one compiled implementation.
//!
//! - **Generic widget / layout code** (no `glow` dependency) →
//!   `demo-ui`
//! - **GL-using demo widgets** (custom shaders, instanced draws,
//!   etc.) → here, in dedicated modules like [`bar_grid`]
//! - **Platform shell (OS window / canvas, event loop, persistence
//!   backend)** → `demo-native` and `demo-wasm`
//!
//! What does **not** belong here: anything that's specific to one
//! platform (winit code, wasm-bindgen exports, file I/O), and any
//! widget that doesn't need direct GL access (those go in `demo-ui`).
//!
//! If a demo widget that uses GL ever ends up living in a platform
//! crate again, native local testing diverges from the deployed WASM
//! build — the exact failure mode that motivated this split.  Keep
//! both shells pointing at the same compiled bytes from this crate.
//!
//! # Pipeline
//!
//! ```text
//! Widget::paint(&mut dyn DrawCtx)
//!   │  path building (move_to/line_to/…) + shape helpers (rect/circle/…)
//!   │  fill() / stroke() → tess2 tessellation → vertex buffer
//!   │  fill_text() → shape_and_flatten_text → tess2 → vertex buffer
//!   ↓
//! GL draw calls (solid-colour GLSL ES 3.0 shader)
//! ```
//!
//! # Coordinate system
//!
//! All incoming coordinates are in **Y-up pixel space**: origin at the
//! bottom-left of the viewport, positive Y upward.  Paths are stored in
//! widget-local coordinates so AGG can expand fills and strokes before the
//! transform stack maps them to screen-space Y-up.  The vertex shader converts
//! screen-space pixels → GL NDC with `ndc = (pos / resolution) * 2 - 1`.

pub mod frame;
pub use frame::{begin_frame, render_app_frame};

mod gl_support;

/// 3-D Animation widget — single source of truth shared between
/// `demo-native` and `demo-wasm`.  See `bar_grid` module docs for the
/// full design rationale.  Both platform crates re-export
/// `GlCubeWidget` and `CUBE_SCREEN_RECT` from here so there is exactly
/// one compiled implementation of the demo's GL widget.
pub mod bar_grid;
pub use bar_grid::{BarGridGlRenderer, GlCubeWidget, CUBE_SCREEN_RECT};

use std::rc::Rc;
use std::sync::{Arc, Weak};

use agg_gui::color::Color;
use agg_gui::draw_ctx::{DrawCtx, FillRule};
use agg_gui::gl_renderer::GlyphCache;
use agg_gui::text::{shape_glyphs, Font, TextMetrics};
use agg_gui::CompOp;
use agg_gui::TransAffine;
use agg_gui::{LineCap, LineJoin};
use agg_rust::arc::Arc as AggArc;
use agg_rust::basics::PATH_FLAGS_NONE;
use agg_rust::conv_curve::ConvCurve;
use agg_rust::conv_dash::ConvDash;
use agg_rust::conv_stroke::ConvStroke;
use agg_rust::conv_transform::ConvTransform;
use agg_rust::path_storage::PathStorage;
use agg_rust::rounded_rect::RoundedRect;
use glow::HasContext;

mod ctx_core;
mod draw_ctx_impl;
mod overlays;
mod shaders;
mod text_render;

pub use overlays::{draw_hover_overlay, draw_status_overlay};

// ---------------------------------------------------------------------------
// GlGfxCtx
// ---------------------------------------------------------------------------

/// One entry in the Arc-keyed GL texture cache.  The `Weak` serves as the
/// liveness sentinel: when all strong refs to the underlying `Vec<u8>` have
/// been dropped (typically because the L1 pixel cache evicted its entry),
/// `weak.upgrade()` returns `None` and the next sweep deletes the texture.
struct ArcTextureEntry {
    weak: Weak<Vec<u8>>,
    texture: glow::Texture,
    w: u32,
    h: u32,
}

#[derive(Clone)]
struct SavedGlDrawState {
    viewport: (f32, f32),
    fill_color: Color,
    stroke_color: Color,
    line_width: f64,
    line_join: LineJoin,
    line_cap: LineCap,
    fill_rule: FillRule,
    miter_limit: f64,
    line_dash: Vec<f64>,
    dash_offset: f64,
    global_alpha: f64,
    state_stack: Vec<(TransAffine, Option<[i32; 4]>)>,
    font: Option<Arc<Font>>,
    font_size: f64,
    lcd_mode: bool,
}

/// One transient GL compositing layer.
struct GlLayerEntry {
    fbo: glow::Framebuffer,
    texture: glow::Texture,
    stencil: glow::Renderbuffer,
    width: i32,
    height: i32,
    origin_x: f64,
    origin_y: f64,
    alpha: f64,
    parent_fbo: Option<glow::Framebuffer>,
    saved: SavedGlDrawState,
}

/// A [`DrawCtx`] that renders via `glow` (WebGL2 or native GL).
///
/// Create once per frame (or share via mutable reference) and pass to
/// [`App::paint`].  After `paint` returns, call [`GlGfxCtx::flush`] to
/// submit any buffered draw calls.  In the current implementation draw calls
/// are submitted immediately in `fill()` / `stroke()`, so `flush` is a no-op
/// placeholder.
pub struct GlGfxCtx {
    gl: Rc<glow::Context>,
    viewport: (f32, f32),

    // GL resources for the solid-colour pipeline (created once, reused every frame)
    prog: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    ibo: glow::Buffer, // persistent index buffer — no per-draw alloc
    res_loc: Option<glow::UniformLocation>,
    color_loc: Option<glow::UniformLocation>,

    // AA solid-colour pipeline — identical to `prog` but with an extra
    // per-vertex `a_alpha` attribute for analytic edge AA (tess2 edge-flag
    // halo strips).  Fills and strokes with AA use this program.
    aa_prog: glow::Program,
    aa_vao: glow::VertexArray,
    aa_vbo: glow::Buffer,
    aa_ibo: glow::Buffer,
    aa_res_loc: Option<glow::UniformLocation>,
    aa_color_loc: Option<glow::UniformLocation>,

    // Textured-quad pipeline (draw_image_rgba — markdown images, screenshots,
    // AGG-rasterised Label backbuffers).
    tex_prog: glow::Program,
    tex_vao: glow::VertexArray,
    tex_vbo: glow::Buffer,
    tex_res_loc: Option<glow::UniformLocation>,
    tex_sampler_loc: Option<glow::UniformLocation>,

    // Premultiplied layer texture compositor.
    layer_prog: glow::Program,
    layer_res_loc: Option<glow::UniformLocation>,
    layer_sampler_loc: Option<glow::UniformLocation>,
    layer_alpha_loc: Option<glow::UniformLocation>,

    // LCD subpixel compositing pipeline (see `LCD_VERT` / `LCD_FRAG`).
    lcd_prog: glow::Program,
    lcd_vao: glow::VertexArray,
    lcd_vbo: glow::Buffer,
    lcd_res_loc: Option<glow::UniformLocation>,
    lcd_sampler_loc: Option<glow::UniformLocation>,
    lcd_color_loc: Option<glow::UniformLocation>,
    /// WASM-only: per-channel selector uniform for the 3-pass
    /// color-masked fallback.  Ignored on desktop (its shader uses
    /// dual-source blend instead).
    #[allow(dead_code)]
    lcd_channel_loc: Option<glow::UniformLocation>,

    // Arc-pointer-keyed texture cache for LCD coverage masks — same
    // pattern as `arc_texture_cache`.  One upload per unique text
    // raster; textures live as long as the mask `Arc` is still strong
    // in `agg_gui::text_lcd`'s LRU cache.
    lcd_arc_texture_cache: std::collections::HashMap<usize, ArcTextureEntry>,

    // LCD BACKBUFFER pipeline (see `LCB_FRAG`) — dual-source per-channel
    // blit of a two-plane `LcdCoverage` cache onto the destination.
    // Reuses `LCD_VERT` because the vertex work is identical.
    lcb_prog: glow::Program,
    lcb_res_loc: Option<glow::UniformLocation>,
    lcb_color_sampler: Option<glow::UniformLocation>,
    lcb_alpha_sampler: Option<glow::UniformLocation>,
    /// WASM-only (mirrors `lcd_channel_loc`).
    #[allow(dead_code)]
    lcb_channel_loc: Option<glow::UniformLocation>,

    // Texture cache keyed on (ptr, len, w, h, head/tail byte hash).  Used by
    // the generic `draw_image_rgba(&[u8], …)` path (markdown images, screenshot
    // display, image widgets).  LRU eviction keeps memory bounded.
    texture_cache: std::collections::HashMap<u64, (glow::Texture, u32, u32)>,
    texture_cache_order: std::collections::VecDeque<u64>,

    // Arc-pointer-keyed texture cache for `draw_image_rgba_arc` — the hot path
    // for `Label` backbuffers (which now live in the crate-level `image_cache`
    // as `Arc<Vec<u8>>`).  Holds a `Weak<Vec<u8>>` per entry; when the Arc is
    // dropped by its owner (the L1 pixel cache, via LRU eviction) the `Weak`
    // fails to upgrade and the next sweep deletes the GL texture.  This is the
    // Rust equivalent of MatterCAD's `ConditionalWeakTable<byte[], ImageTexturePlugin>`
    // finalizer → deferred-delete pattern.
    arc_texture_cache: std::collections::HashMap<usize, ArcTextureEntry>,

    // Transient FBO layer stack used for windows and other whole-subtree effects.
    layer_stack: Vec<GlLayerEntry>,
    current_fbo: Option<glow::Framebuffer>,

    // Drawing state
    fill_color: Color,
    stroke_color: Color,
    line_width: f64,
    line_join: LineJoin,
    line_cap: LineCap,
    fill_rule: FillRule,
    miter_limit: f64,
    line_dash: Vec<f64>,
    dash_offset: f64,
    global_alpha: f64,

    // State stack: each entry holds a saved (transform, clip) pair.
    // The last entry is the current state.
    // clip: GL scissor rect (x, y_down, w, h), or None = no scissor.
    state_stack: Vec<(TransAffine, Option<[i32; 4]>)>,

    // Path builder — stored in local Y-up coordinates. Fill/stroke convert
    // through AGG, then apply the current transform before tessellating.
    path: PathStorage,

    // Font
    font: Option<Arc<Font>>,
    font_size: f64,

    // Glyph vertex cache — survives frame resets, populated on first use.
    glyph_cache: GlyphCache,

    /// LCD mode for this ctx — see `GfxCtx::lcd_mode`.  Set by the main
    /// render loop each frame from `font_settings::lcd_enabled()`.
    lcd_mode: bool,
}
