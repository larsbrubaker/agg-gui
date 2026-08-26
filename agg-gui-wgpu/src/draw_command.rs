//! Deferred draw-command list — the enum `WgpuGfxCtx` accumulates during
//! `fill()` / `stroke()` / `fill_text()` and flushes in `end_frame()`.
//!
//! Split out of `lib.rs` to keep that file under the project's 800-line limit.
//! `end_frame_prepare` turns each of these into a `Prepared` entry (GPU buffers
//! + bind groups) and `end_frame` executes them inside render passes.

use std::sync::Arc;

use agg_gui::color::Color;

use crate::custom_render::SharedCustomRenderer;
use crate::gradient;
use crate::LayerRoundedClip;

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
    /// Generic custom-render hook — dispatches to user code implementing
    /// [`WgpuCustomRender`].  The executor ends the active 2-D pass, lets the
    /// renderer record its own pass(es) onto the active layer or surface, then
    /// reopens the 2-D pass with `LoadOp::Load`.  Pushed via
    /// [`WgpuGfxCtx::push_custom_render`].
    Custom {
        renderer: SharedCustomRenderer,
        screen_rect: agg_gui::Rect,
        parent_clip: Option<[i32; 4]>,
    },
}
