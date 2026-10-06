//! wgpu renderer for agg-gui's tumble cube
//! ([`agg_gui::widgets::tumble_cube::TumbleCube`]).
//!
//! Install with `TumbleCube::new(camera).with_gpu_renderer(Box::new(
//! WgpuTumbleCubeRenderer::new()))`.  On a [`WgpuGfxCtx`] it queues a
//! custom render; on any other context it declines and the widget falls
//! back to its software ray-caster.
//!
//! The draw mirrors MatterCAD's `TumbleCubeControl.OnDraw` on the WebGPU
//! scene renderer: the cube mesh goes through MatterCAD's own scene module
//! (`NodeDesignerScene.wgsl`, copied unchanged next to this file —
//! `sceneVertexMain` + `sceneTextureMain`) with the default `LightingData`
//! rig, into a 3x supersampled offscreen target (the C#
//! `BeginFullFrameCapture`) that is box-downsampled onto the frame.  The
//! downsample uses agg-gui-wgpu's existing [`SsaaFramebuffer`] 3x3 box blit
//! rather than MatterCAD's post-process module, which needs that renderer's
//! whole post-process bind layout for the one entry point.
//! The pipeline / GPU resource code is adapted from AtomArtist's
//! `tumble_cube/renderer.rs`; its own shader text is not used.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::widgets::tumble_cube::geometry::{CubeLighting, FACE_FRAMES};
use agg_gui::widgets::tumble_cube::{CubeView, TumbleCubeFrame, TumbleCubeGpuRenderer};

use crate::custom_render::{SharedCustomRenderer, WgpuCustomRender, WgpuCustomRenderCtx};
use crate::ssaa::SsaaFramebuffer;
use crate::WgpuGfxCtx;

mod gpu;

/// MatterCAD's scene module, verbatim (see the file's header).
pub(crate) const SCENE_WGSL: &str = include_str!("NodeDesignerScene.wgsl");

/// Linear supersample factor — the C# full-frame capture is 3x.
const SSAA: u32 = 3;

/// The [`TumbleCubeGpuRenderer`] for wgpu.  Cheap to create; GPU resources
/// are built on first use and kept across frames.
#[derive(Default)]
pub struct WgpuTumbleCubeRenderer {
    shared: Rc<RefCell<CubeGpu>>,
}

impl WgpuTumbleCubeRenderer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TumbleCubeGpuRenderer for WgpuTumbleCubeRenderer {
    fn paint(&mut self, ctx: &mut dyn DrawCtx, frame: &TumbleCubeFrame<'_>) -> bool {
        let t = ctx.transform();
        let Some(any) = ctx.as_any_mut() else {
            return false;
        };
        let Some(wgpu_ctx) = any.downcast_mut::<WgpuGfxCtx>() else {
            return false;
        };
        {
            let mut s = self.shared.borrow_mut();
            s.view = Some(frame.view);
            if s.faces_version != Some(frame.faces_version) {
                // Hand the renderer this frame's pixels; the upload happens
                // in `render`, where the queue is available.
                s.pending_faces = Some(frame.faces.iter().map(|f| f.active.clone()).collect());
                s.faces_version = Some(frame.faces_version);
            }
        }
        let (w, h) = (frame.view.width, frame.view.height);
        let (mut x0, mut y0, mut x1, mut y1) = (0.0, 0.0, w, h);
        t.transform(&mut x0, &mut y0);
        t.transform(&mut x1, &mut y1);
        let rect = agg_gui::Rect::new(x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs());
        let shared: SharedCustomRenderer = self.shared.clone();
        wgpu_ctx.push_custom_render(shared, rect);
        true
    }
}

/// State shared between the widget-side hook and the deferred render.
#[derive(Default)]
struct CubeGpu {
    view: Option<CubeView>,
    pending_faces: Option<Vec<Arc<Vec<u8>>>>,
    faces_version: Option<u64>,
    state: Option<gpu::GpuState>,
    framebuffer: Option<SsaaFramebuffer>,
}

impl WgpuCustomRender for CubeGpu {
    fn render(&mut self, ctx: WgpuCustomRenderCtx<'_>) {
        let Some(view) = self.view else { return };
        let pw = ctx.screen_rect.width.round().max(1.0) as u32 * SSAA;
        let ph = ctx.screen_rect.height.round().max(1.0) as u32 * SSAA;
        if !matches!(&self.state, Some(s) if s.format == ctx.surface_format) {
            self.state = Some(gpu::GpuState::new(ctx.device, ctx.surface_format));
            // A rebuilt state has blank textures: re-upload.
            self.faces_version = None;
        }
        let Some(state) = self.state.as_mut() else {
            return;
        };
        if let Some(faces) = self.pending_faces.take() {
            state.upload_faces(ctx.queue, &faces);
        }
        match &mut self.framebuffer {
            Some(fb) => fb.ensure_size(ctx.device, pw, ph),
            None => {
                self.framebuffer = Some(SsaaFramebuffer::new(
                    ctx.device,
                    pw,
                    ph,
                    ctx.surface_format,
                    true,
                ));
            }
        }
        let Some(fb) = self.framebuffer.as_ref() else {
            return;
        };
        let Some(depth) = fb.depth_view() else { return };
        state.write_uniforms(ctx.queue, &view, pw, ph);
        state.draw(ctx.encoder, fb.render_view(), depth, pw, ph);
        fb.blit_downsample_3x_to(
            ctx.device,
            ctx.encoder,
            ctx.target_view,
            ctx.target_size,
            ctx.screen_rect,
            ctx.parent_clip,
            ctx.pipelines,
        );
    }
}

/// Cube vertices in the scene module's `VertexInput` layout: position,
/// normal, texCoord, edgeHints (unused: no wireframe), vertexColor.
/// Texture V is flipped because the face images are stored top row first.
pub(crate) fn cube_vertices() -> (Vec<f32>, Vec<u32>) {
    let mut v = Vec::with_capacity(24 * 15);
    let mut idx = Vec::with_capacity(36);
    let uvs = [(0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)];
    for (face, frame) in FACE_FRAMES.iter().enumerate() {
        let base = (face * 4) as u32;
        for (corner, uv) in frame.corners().iter().zip(uvs) {
            v.extend(corner.iter().map(|c| *c as f32));
            v.extend(frame.normal.iter().map(|c| *c as f32));
            v.extend([uv.0, uv.1, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        }
        idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (v, idx)
}

/// The `Lights` uniform block for agg-sharp's default rig, as
/// `WebGpuSceneRenderer.WriteLightUniform` lays it out (8 vec4s).
pub(crate) fn lights_block() -> [f32; 32] {
    let l = CubeLighting::default();
    let d0 = l.light0_direction;
    let d1 = l.light1_direction;
    let (a0, a1) = (l.sky_ambient as f32, l.ground_ambient as f32);
    let (f0, f1) = (l.light0_diffuse as f32, l.light1_diffuse as f32);
    [
        d0[0] as f32,
        d0[1] as f32,
        d0[2] as f32,
        0.0, //
        a0,
        a0,
        a0,
        1.0, //
        f0,
        f0,
        f0,
        1.0, //
        d1[0] as f32,
        d1[1] as f32,
        d1[2] as f32,
        0.0, //
        a1,
        a1,
        a1,
        1.0, //
        f1,
        f1,
        f1,
        1.0, //
        1.0,
        1.0,
        0.0,
        0.0, // both lights on
        0.0,
        32.0,
        0.0,
        0.0, // no specular, default power, no rim
    ]
}

/// `SceneEffect` for a plain white textured draw: mesh colour white, no
/// wireframe, lit, mesh (not vertex) colour, alpha multiplier 1.
pub(crate) fn effect_block(width: u32, height: u32) -> [f32; 52] {
    let mut e = [0.0f32; 52];
    e[0..4].copy_from_slice(&[1.0, 1.0, 1.0, 1.0]);
    e[12..16].copy_from_slice(&[width as f32, height as f32, 0.0, 0.0]);
    e[16..20].copy_from_slice(&[0.0, 1.0, 0.0, 0.0]);
    e
}

/// Row-major flatten: the module multiplies `mat * v`, so writing a
/// row-vector matrix row by row is the transpose it expects (see the
/// module's rule 1).
pub(crate) fn rows(m: [[f64; 4]; 4]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for r in 0..4 {
        for c in 0..4 {
            out[r * 4 + c] = m[r][c] as f32;
        }
    }
    out
}

#[cfg(test)]
#[path = "readback_tests.rs"]
mod readback_tests;
