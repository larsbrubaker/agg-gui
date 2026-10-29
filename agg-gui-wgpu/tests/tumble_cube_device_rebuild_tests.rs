//! The wgpu tumble cube keeps drawing after its device is lost and rebuilt,
//! and when one cube is drawn more than once in a frame.
//!
//! A shell that loses its GPU device (a browser tab out of GPU memory, a
//! driver reset) builds a new device and a new `WgpuGfxCtx`, but the widget
//! tree, and with it the cube's renderer and the GPU state it built, stays.
//! Every resource that state holds belongs to the lost device; recording
//! them into the new device's encoder is a validation error, so every frame
//! after the rebuild is thrown away. The renderer must notice the new device
//! generation (`WgpuCustomRenderCtx::device_generation`) and build again.
//!
//! Drawing the same cube twice before the frame is submitted must not let the
//! second draw destroy buffers the first one recorded: the submit would then
//! reject the whole frame.
//!
//! Skipped (passes trivially) when no GPU adapter is available.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::widget::Widget;
use agg_gui::widgets::tumble_cube::math::{self, Mat3};
use agg_gui::widgets::tumble_cube::{TumbleCube, TumbleCubeCamera};
use agg_gui::{Point, Size};
use agg_gui_wgpu::{HeadlessFrame, HeadlessGpu, WgpuTumbleCubeRenderer};

/// The frame; the cube widget is 100 x 100 at its bottom left.
const SIZE: u32 = 128;
const CUBE: u32 = 100;
const CLEAR: Color = Color::rgba(0.0, 1.0, 0.0, 1.0);

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

/// Render one `width` x SIZE frame on `gpu` painted by `paint` and return its
/// pixels (rows top first), failing the test on any validation error the
/// device reports.
fn frame_on(gpu: &HeadlessGpu, width: u32, paint: impl FnOnce(&mut dyn DrawCtx)) -> Vec<u8> {
    let errors: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink = Arc::clone(&errors);
    gpu.device()
        .on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            sink.lock().expect("error list").push(e.to_string());
        }));
    let mut frame = HeadlessFrame::new(gpu, width, SIZE);
    frame.render(CLEAR, |ctx| paint(ctx));
    let pixels = frame.read_rgba().expect("read the frame back");
    let errors = errors.lock().expect("error list");
    assert!(
        errors.is_empty(),
        "the cube's frame must be valid on device generation {}: {errors:?}",
        gpu.device_generation()
    );
    pixels.clone()
}

/// The pixel in the middle of a cube drawn `left` pixels from the frame's
/// left edge, at its bottom.
fn cube_middle(pixels: &[u8], width: u32, left: u32) -> [u8; 4] {
    let (x, y_down) = (left + CUBE / 2, SIZE - CUBE / 2);
    let i = ((y_down * width + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

/// Paint `cube` in one frame on `gpu` and return the middle pixel.
fn middle_pixel_on(gpu: &HeadlessGpu, cube: &mut TumbleCube) -> [u8; 4] {
    let pixels = frame_on(gpu, SIZE, |ctx| cube.paint(ctx));
    cube_middle(&pixels, SIZE, 0)
}

fn new_cube() -> TumbleCube {
    let cam = Rc::new(RefCell::new(FixedCamera(math::look_at(
        [0.4, 1.0, -0.6],
        [0.0, 0.0, 1.0],
    ))));
    let mut cube = TumbleCube::new(cam).with_gpu_renderer(Box::new(WgpuTumbleCubeRenderer::new()));
    cube.layout(Size::new(SIZE as f64, SIZE as f64));
    cube
}

#[test]
fn one_cube_drawn_twice_in_a_frame_draws_both() {
    let gpu = match HeadlessGpu::create() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    let mut cube = new_cube();
    let alone = middle_pixel_on(&gpu, &mut cube);
    let width = 2 * SIZE;
    let pixels = frame_on(&gpu, width, |ctx| {
        cube.paint(ctx);
        ctx.save();
        ctx.translate(SIZE as f64, 0.0);
        cube.paint(ctx);
        ctx.restore();
    });
    assert_eq!(cube_middle(&pixels, width, 0), alone, "the first draw");
    assert_eq!(cube_middle(&pixels, width, SIZE), alone, "the second draw");
}

#[test]
fn the_cube_draws_again_on_a_rebuilt_device() {
    let first = match HeadlessGpu::create() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    let mut cube = new_cube();
    let before = middle_pixel_on(&first, &mut cube);
    assert_ne!(
        before,
        [0, 255, 0, 255],
        "the cube draws on the first device"
    );

    // Lose the device, then rebuild as a shell recovers: a new device on a
    // new instance, with the same widget tree.
    first.device().destroy();
    let rebuilt = HeadlessGpu::create().expect("rebuild the device after the loss");
    assert_ne!(rebuilt.device_generation(), first.device_generation());
    let after = middle_pixel_on(&rebuilt, &mut cube);
    assert_eq!(
        after, before,
        "the cube draws the same picture on the rebuilt device"
    );
}
