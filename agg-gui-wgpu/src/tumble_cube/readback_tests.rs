//! Headless GPU readback tests for [`super::WgpuTumbleCubeRenderer`].
//!
//! They prove MatterCAD's scene module compiles and binds against our
//! layout, and that the GPU cube agrees with agg-gui's software renderer
//! (same camera, lighting and textures), that its faces are uploaded
//! once, not again on the paint after the first, and that frame after frame
//! it destroys the buffers it makes instead of leaving them to the browser's
//! garbage collector.  Like the other readback tests
//! they pass trivially when no GPU adapter is available.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::widget::Widget;
use agg_gui::widgets::tumble_cube::math::{self, Mat3};
use agg_gui::widgets::tumble_cube::{cpu_render, TumbleCube, TumbleCubeCamera};
use agg_gui::{Point, Size};

use super::WgpuTumbleCubeRenderer;
use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::WgpuGfxCtx;

const SIZE: u32 = 128;

struct FixedCamera(Mat3);

impl TumbleCubeCamera for FixedCamera {
    fn view_rotation(&self) -> Mat3 {
        self.0
    }
    fn begin_rotate(&mut self, _: Point) {}
    fn rotate(&mut self, _: Point) {}
    fn end_rotate(&mut self) {}
    fn animate_rotation(&mut self, _: Mat3) {}
}

fn render_cube(rotation: Mat3) -> Option<(Vec<u8>, TumbleCube)> {
    let (device, queue) = try_device()?;
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), SIZE, SIZE);
    let mut ctx = WgpuGfxCtx::new(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        SIZE as f32,
        SIZE as f32,
    );
    ctx.reset(SIZE as f32, SIZE as f32);
    ctx.clear(Color::rgba(0.0, 1.0, 0.0, 1.0));
    let cam = Rc::new(RefCell::new(FixedCamera(rotation)));
    let mut cube = TumbleCube::new(cam).with_gpu_renderer(Box::new(WgpuTumbleCubeRenderer::new()));
    cube.layout(Size::new(SIZE as f64, SIZE as f64));
    cube.paint(&mut ctx);
    ctx.flush_to_surface(&target.view);
    Some((target.read(), cube))
}

#[test]
fn gpu_cube_matches_the_software_render() {
    // An oblique view so three faces, their lighting and the silhouette
    // are all exercised.
    let rotation = math::look_at([0.4, 1.0, -0.6], [0.0, 0.0, 1.0]);
    let Some((gpu, cube)) = render_cube(rotation) else {
        return;
    };
    let view = cube.view();
    let cpu = cpu_render::render(&view, &cube.faces().faces, 100, 100);
    // The widget is 100x100 at the bottom-left of the 128x128 target;
    // compare away from silhouette edges and label glyphs.
    let mut compared = 0;
    for y_down in (5..95).step_by(9) {
        for x in (5..95).step_by(9) {
            let c = &cpu[(y_down * 100 + x) * 4..][..4];
            let g = px(&gpu, SIZE, x as u32, SIZE - 100 + y_down as u32);
            if c[3] == 0 {
                assert_eq!(
                    g,
                    [0, 255, 0, 255],
                    "outside the cube at ({x}, {y_down}) must show the clear colour"
                );
                continue;
            }
            if c[3] < 255 {
                continue; // silhouette edge: AA differs between renderers
            }
            compared += 1;
            for ch in 0..3 {
                let d = (c[ch] as i32 - g[ch] as i32).abs();
                assert!(d <= 24, "({x}, {y_down}) cpu {c:?} gpu {g:?}");
            }
        }
    }
    assert!(
        compared > 20,
        "the cube must cover the middle of the widget"
    );
}

#[test]
fn the_faces_are_uploaded_once_across_frames() {
    let Some((device, queue)) = try_device() else {
        return;
    };
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), SIZE, SIZE);
    let mut ctx = WgpuGfxCtx::new(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        SIZE as f32,
        SIZE as f32,
    );
    let renderer = WgpuTumbleCubeRenderer::new();
    let shared = Rc::clone(&renderer.shared);
    let cam = Rc::new(RefCell::new(FixedCamera(math::look_at(
        [0.4, 1.0, -0.6],
        [0.0, 0.0, 1.0],
    ))));
    let mut cube = TumbleCube::new(cam).with_gpu_renderer(Box::new(renderer));
    cube.layout(Size::new(SIZE as f64, SIZE as f64));
    // The first frame makes the GPU state and uploads the faces; the next ones, with the faces
    // unchanged, upload nothing.
    for _ in 0..3 {
        ctx.reset(SIZE as f32, SIZE as f32);
        ctx.clear(Color::rgba(0.0, 1.0, 0.0, 1.0));
        cube.paint(&mut ctx);
        ctx.flush_to_surface(&target.view);
    }
    assert_eq!(shared.borrow().face_uploads, 1);
}

/// Every frame the cube blits its supersampled picture with two buffers made
/// for that draw. In the browser a dropped buffer is freed only when the
/// garbage collector runs, which a browser feeling no GPU-memory pressure may
/// put off until the device runs out of memory, so each frame's pair must be
/// destroyed once that frame is submitted: the number alive stays flat
/// however many frames are painted.
#[test]
fn painting_frames_leaves_no_buffers_or_textures_behind() {
    let Some((device, queue)) = try_device() else {
        return;
    };
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), SIZE, SIZE);
    let mut ctx = WgpuGfxCtx::new(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        SIZE as f32,
        SIZE as f32,
    );
    let renderer = WgpuTumbleCubeRenderer::new();
    let shared = Rc::clone(&renderer.shared);
    let cam = Rc::new(RefCell::new(FixedCamera(math::look_at(
        [0.4, 1.0, -0.6],
        [0.0, 0.0, 1.0],
    ))));
    let mut cube = TumbleCube::new(cam).with_gpu_renderer(Box::new(renderer));
    cube.layout(Size::new(SIZE as f64, SIZE as f64));
    let mut first_textures = None;
    for frame in 1..=20u64 {
        ctx.reset(SIZE as f32, SIZE as f32);
        ctx.clear(Color::rgba(0.0, 1.0, 0.0, 1.0));
        cube.paint(&mut ctx);
        ctx.flush_to_surface(&target.view);
        let gpu = shared.borrow();
        let fb = gpu.framebuffer.as_ref().expect("the cube drew");
        let buffers = ctx.buffer_counts();
        // Each frame's blit handed its two buffers to the context, which
        // destroyed them after the frame's submit.
        assert_eq!(buffers.let_go, 2 * frame, "frame {frame}: {buffers:?}");
        assert_eq!(buffers.waiting(), 0, "frame {frame}: {buffers:?}");
        let textures = (fb.texture_counts(), ctx.texture_counts().held());
        assert_eq!(
            textures.0.held(),
            2,
            "frame {frame}: colour and depth: {textures:?}"
        );
        let first = *first_textures.get_or_insert(textures.1);
        assert_eq!(
            textures.1, first,
            "frame {frame}: the context holds the same textures: {textures:?}"
        );
    }
}
