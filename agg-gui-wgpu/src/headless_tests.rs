//! Tests for the public offscreen renderer in `headless.rs`: a widget tree
//! painted through [`HeadlessFrame::render_app`] lands in the read-back where
//! layout put it, custom render passes run inside a headless frame, the
//! read-back is RGBA in both row orders at widths whose rows need padding,
//! and every frame shares one device.
//!
//! Like the crate's other GPU tests they skip (pass trivially) when the
//! machine has no adapter. `app_frame_at_1280x800_with_readback` prints the
//! cost of a full-window frame plus read-back (run with `--nocapture`).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::text::Font;
use agg_gui::{App, Container, Insets, Label, Rect, Size};

use crate::custom_render::{SharedCustomRenderer, WgpuCustomRender, WgpuCustomRenderCtx};
use crate::headless::{HeadlessFrame, HeadlessGpu, HEADLESS_FORMAT};

/// The shared device, or `None` (the test skips) when there is no adapter.
fn gpu() -> Option<&'static HeadlessGpu> {
    match HeadlessGpu::shared() {
        Ok(gpu) => Some(gpu),
        Err(e) => {
            eprintln!("SKIP: {e}");
            None
        }
    }
}

/// Pixel `(x, y)` of a top-row-first RGBA read-back.
fn px(data: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [data[i], data[i + 1], data[i + 2], data[i + 3]]
}

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

fn color(c: [u8; 4]) -> Color {
    Color::rgba(
        c[0] as f32 / 255.0,
        c[1] as f32 / 255.0,
        c[2] as f32 / 255.0,
        c[3] as f32 / 255.0,
    )
}

/// A laid-out `App` paints through `render_app` where its layout put each
/// widget: a red child Container whose margins leave it the top-left quadrant
/// of a green root. The 100-pixel width needs padded read-back rows, and the
/// colours prove the BGRA target comes back in RGBA order.
#[test]
fn render_app_paints_the_widget_tree_where_layout_put_it() {
    let Some(gpu) = gpu() else { return };
    let (w, h) = (100u32, 60u32);
    let child = Container::new()
        .with_background(color(RED))
        .with_margin(Insets {
            left: 0.0,
            right: w as f64 / 2.0,
            top: 0.0,
            bottom: h as f64 / 2.0,
        });
    let root = Container::new()
        .with_background(color(GREEN))
        .add(Box::new(child));
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(w as f64, h as f64));

    let mut frame = HeadlessFrame::new(gpu, w, h);
    assert_eq!(frame.target().format(), HEADLESS_FORMAT);
    frame.render_app(Color::white(), &mut app);
    let data = frame.read_rgba().expect("read back");
    assert_eq!(data.len(), (w * h * 4) as usize, "rows come back unpadded");

    // Visual top-left quadrant (top rows, left columns) is the red child.
    assert_eq!(px(&data, w, 10, 10), RED, "inside the child");
    assert_eq!(
        px(&data, w, w / 2 - 2, h / 2 - 2),
        RED,
        "child's inner corner"
    );
    // Everything else is the green root.
    assert_eq!(px(&data, w, w / 2 + 2, 10), GREEN, "right of the child");
    assert_eq!(px(&data, w, 10, h / 2 + 2), GREEN, "below the child");
    assert_eq!(px(&data, w, w - 1, h - 1), GREEN, "bottom-right corner");
}

/// `read_framebuffer` is the software renderer's layout (row 0 = y 0 at the
/// bottom); `read_rgba` is a PNG's (row 0 at the top). A bar painted along the
/// bottom edge in y-up coordinates is the first framebuffer row and the last
/// read-back row.
#[test]
fn read_framebuffer_is_bottom_row_first_and_read_rgba_top_row_first() {
    let Some(gpu) = gpu() else { return };
    let (w, h) = (70u32, 40u32);
    let mut frame = HeadlessFrame::new(gpu, w, h);
    frame.render(color(GREEN), |ctx| {
        ctx.set_fill_color(color(BLUE));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w as f64, 8.0);
        ctx.fill();
    });

    let top_down = frame.read_rgba().expect("read back");
    assert_eq!(px(&top_down, w, 5, h - 1), BLUE, "last row is the bottom");
    assert_eq!(px(&top_down, w, 5, 0), GREEN, "first row is the top");

    let fb = frame.read_framebuffer().expect("read back");
    assert_eq!((fb.width(), fb.height()), (w, h));
    assert_eq!(px(fb.pixels(), w, 5, 0), BLUE, "framebuffer row 0 is y = 0");
    assert_eq!(
        px(fb.pixels(), w, 5, h - 1),
        GREEN,
        "last framebuffer row is the top"
    );
}

/// The target size and format a custom pass was handed.
type SeenTarget = Option<((u32, u32), wgpu::TextureFormat)>;

/// Clears its target to one colour and records what the frame handed it.
struct ClearPass {
    clear: wgpu::Color,
    seen: Rc<Cell<SeenTarget>>,
}

impl WgpuCustomRender for ClearPass {
    fn render(&mut self, ctx: WgpuCustomRenderCtx<'_>) {
        self.seen.set(Some((ctx.target_size, ctx.surface_format)));
        drop(ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("headless-test-clear-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.target_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(self.clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
    }
}

/// A custom render pass queued during a headless frame (a 3-D view's path)
/// runs on the frame's target at the frame's size and format, between the 2-D
/// content queued before and after it.
#[test]
fn custom_render_runs_inside_a_headless_frame() {
    let Some(gpu) = gpu() else { return };
    let (w, h) = (64u32, 32u32);
    let seen = Rc::new(Cell::new(None));
    let pass: SharedCustomRenderer = Rc::new(std::cell::RefCell::new(ClearPass {
        clear: wgpu::Color::BLUE,
        seen: Rc::clone(&seen),
    }));

    let mut frame = HeadlessFrame::new(gpu, w, h);
    frame.render(color(GREEN), |ctx| {
        ctx.push_custom_render(pass, Rect::new(0.0, 0.0, w as f64, h as f64));
        ctx.set_fill_color(color(RED));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 16.0, 8.0);
        ctx.fill();
    });
    let data = frame.read_rgba().expect("read back");

    assert_eq!(seen.get(), Some(((w, h), HEADLESS_FORMAT)));
    assert_eq!(
        px(&data, w, w - 1, 0),
        BLUE,
        "the custom pass covered the clear"
    );
    assert_eq!(
        px(&data, w, 4, h - 2),
        RED,
        "2-D after the pass draws on top"
    );
}

/// Every frame runs on the one shared device: asking again returns the same
/// `HeadlessGpu`, two frames' contexts hold the same device, and each frame's
/// target still reads back only its own pixels.
#[test]
fn second_frame_reuses_the_shared_device() {
    let Some(gpu) = gpu() else { return };
    let again = HeadlessGpu::shared().expect("the shared device exists");
    assert!(std::ptr::eq(gpu, again), "one HeadlessGpu per process");

    let mut first = HeadlessFrame::new(gpu, 32, 16);
    let mut second = HeadlessFrame::new(again, 48, 24);
    assert!(Arc::ptr_eq(first.ctx().device(), second.ctx().device()));
    assert!(Arc::ptr_eq(first.ctx().device(), gpu.device()));

    first.render(color(RED), |_| {});
    second.render(color(BLUE), |_| {});
    let a = first.read_rgba().expect("read back");
    let b = second.read_rgba().expect("read back");
    assert!(
        a.chunks_exact(4).all(|p| p == RED),
        "first frame is all red"
    );
    assert!(
        b.chunks_exact(4).all(|p| p == BLUE),
        "second frame is all blue"
    );

    // A resized frame keeps its context (and so its device) and reads back
    // at the new size.
    let device = Arc::as_ptr(first.ctx().device());
    first.resize(40, 20);
    assert_eq!(first.size(), (40, 20));
    assert_eq!(Arc::as_ptr(first.ctx().device()), device);
    first.render(color(GREEN), |_| {});
    let c = first.read_rgba().expect("read back");
    assert_eq!(c.len(), 40 * 20 * 4);
    assert!(c.chunks_exact(4).all(|p| p == GREEN));
}

/// What a test harness pays per frame: a 1280x800 `App` (a root and forty
/// text rows) rendered and read back. Prints context creation, the first
/// frame (pipeline and glyph warm-up) and the mean of the steady frames.
#[test]
fn app_frame_at_1280x800_with_readback() {
    // The device is per process: this is its creation cost when this test
    // runs alone (as under nextest), and ~0 when another test made it first.
    let started = Instant::now();
    let Some(gpu) = gpu() else { return };
    let device = started.elapsed();
    let (w, h) = (1280u32, 800u32);
    let font = Arc::new(
        Font::from_slice(include_bytes!("../assets/fonts/NotoSans-Regular.ttf"))
            .expect("test font"),
    );
    let mut root = Container::new().with_background(color(GREEN));
    for i in 0..40 {
        let row = Label::new(format!("Row {i}: the quick brown fox"), Arc::clone(&font));
        root = root.add(Box::new(row));
    }
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(w as f64, h as f64));

    let started = Instant::now();
    let mut frame = HeadlessFrame::new(gpu, w, h);
    let create = started.elapsed();

    // (render, read-back) wall time; the read-back waits for the GPU to
    // finish the frame, so it carries the GPU time too.
    let mut frame_and_read = || {
        let started = Instant::now();
        frame.render_app(Color::white(), &mut app);
        let rendered = started.elapsed();
        let data = frame.read_rgba().expect("read back");
        (rendered, started.elapsed() - rendered, data)
    };
    let (first_render, first_read, data) = frame_and_read();
    assert_eq!(data.len(), (w * h * 4) as usize);
    assert_eq!(
        px(&data, w, w - 1, h - 1),
        GREEN,
        "the root fills the frame"
    );

    const STEADY: u32 = 5;
    let (mut render, mut read) = (Duration::ZERO, Duration::ZERO);
    for _ in 0..STEADY {
        let (r, b, _) = frame_and_read();
        render += r;
        read += b;
    }
    eprintln!(
        "headless 1280x800 on {:?} ({}): device {device:?}; context {create:?}; first frame \
         {first_render:?} + read-back {first_read:?}; steady frame {:?} + read-back {:?}",
        gpu.adapter_info().backend,
        gpu.adapter_info().name,
        render / STEADY,
        read / STEADY
    );
}
