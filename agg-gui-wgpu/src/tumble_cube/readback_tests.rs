//! Headless GPU readback tests for [`super::WgpuTumbleCubeRenderer`].
//!
//! They prove MatterCAD's scene module compiles and binds against our
//! layout, and that the GPU cube agrees with agg-gui's software renderer
//! (same camera, lighting and textures).  Like the other readback tests
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
