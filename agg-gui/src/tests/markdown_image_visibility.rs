//! End-to-end: a remote Markdown image that is on screen starts loading at
//! HiDPI wherever it sits in the viewport, and one that is off screen does not.
//!
//! `MarkdownView` only fetches a remote image (`![alt](http://…)`) once paint
//! finds the image rect on screen: `is_rect_visible_in_root` in
//! `widgets/markdown.rs`, called from `widgets/markdown/paint.rs` to gate
//! `image_loader::load_remote_image`. That check maps the rect through
//! `DrawCtx::root_transform`, which yields root-target DEVICE pixels, so the
//! viewport it compares against must be in device pixels too — while
//! `widget::current_viewport()` is LOGICAL (`App::layout` divides by the
//! effective scale). This suite drives the real `App` layout/paint path at
//! device scale 2 (alone and with a 1.5 UX scale) with the image in the
//! right/top part of the screen, where its device coordinates exceed the
//! logical viewport, and serves a real PNG from a loopback HTTP server so the
//! production fetch path (`http_fetch.rs`) runs. A scale-1 control and an
//! off-screen negative case pin that the culling itself keeps working.

use super::*;
use crate::draw_ctx::DrawCtx;
use crate::text::Font;
use crate::{Event, EventResult, MarkdownView, Rect};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Logical viewport.
const VP_W: f64 = 400.0;
const VP_H: f64 = 300.0;

/// Side of the served square PNG, in image pixels (drawn 1:1 in logical units
/// because it is narrower than the markdown's wrap width).
const IMG_PX: u32 = 40;

/// Generous bound for the loopback fetch + decode round trip.
const LOAD_TIMEOUT: Duration = Duration::from_secs(10);

/// Restores the thread-local device and UX scales even if an assertion fails.
struct ScaleGuard;
impl Drop for ScaleGuard {
    fn drop(&mut self) {
        crate::set_device_scale(1.0);
        crate::ux_scale::set_ux_scale(1.0);
    }
}

/// A one-shot loopback HTTP server: `url` serves `IMG_PX`² of opaque green
/// PNG, and `requests` receives a message when a request arrives.
struct ImageServer {
    url: String,
    requests: mpsc::Receiver<()>,
}

fn green_png() -> Vec<u8> {
    let rgba = [0u8, 255, 0, 255].repeat((IMG_PX * IMG_PX) as usize);
    crate::screenshot::encode_png_rgba(&rgba, IMG_PX, IMG_PX).expect("encode test PNG")
}

/// Serve one 200 response carrying a green PNG (pattern from the
/// `http_fetch.rs` tests). If no request ever comes, the accept thread just
/// stays parked until the test process exits.
fn serve_png_once() -> ImageServer {
    let body = green_png();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let (tx, requests) = mpsc::channel();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = tx.send(());
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    ImageServer {
        url: format!("http://{addr}/img.png"),
        requests,
    }
}

/// App root: fills the viewport and pins its single child, at its natural
/// size for `slot`'s width/height, with its bottom-left at `slot`'s origin.
struct Placer {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    slot: Rect,
}

impl Widget for Placer {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        let slot = self.slot;
        let child = &mut self.children[0];
        let size = child.layout(Size::new(slot.width, slot.height));
        child.set_bounds(Rect::new(slot.x, slot.y, size.width, size.height));
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// An app whose only content is a `MarkdownView` holding one remote image,
/// placed at `slot` (logical, Y-up).
fn markdown_app(url: &str, slot: Rect) -> App {
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
    let view = MarkdownView::new(format!("![remote]({url})"), font);
    App::new(Box::new(Placer {
        bounds: Rect::default(),
        children: vec![Box::new(view)],
        slot,
    }))
}

/// Physical (root target) size of the logical viewport at the current scale.
fn physical_viewport() -> Size {
    let scale = crate::ux_scale::effective_scale();
    Size::new((VP_W * scale).round(), (VP_H * scale).round())
}

/// One host frame: lay out at `phys` and paint into a fresh white target.
fn frame(app: &mut App, phys: Size) -> Framebuffer {
    app.layout(phys);
    let mut fb = Framebuffer::new(phys.width as u32, phys.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        app.paint(&mut ctx);
    }
    fb
}

fn is_green(p: [u8; 4]) -> bool {
    p[1] > 200 && p[0] < 60 && p[2] < 60
}

/// Bounding box `(min_x, min_y, max_x, max_y)` of the green (image) pixels,
/// in root device pixels, Y-up; `None` if the image is not on screen.
fn green_bbox(fb: &Framebuffer) -> Option<(u32, u32, u32, u32)> {
    let mut bbox: Option<(u32, u32, u32, u32)> = None;
    for y in 0..fb.height() {
        for x in 0..fb.width() {
            if is_green(sample(fb, x, y)) {
                bbox = Some(match bbox {
                    None => (x, y, x, y),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                });
            }
        }
    }
    bbox
}

/// Lay out and paint a markdown image at `slot` under the given scales, then
/// require that the fetch starts, the server is asked for the image, and the
/// decoded pixels land at the markdown's position at the right size.
fn assert_on_screen_image_loads(label: &str, device_scale: f64, ux_scale: f64, slot: Rect) {
    crate::set_device_scale(device_scale);
    crate::ux_scale::set_ux_scale(ux_scale);
    let scale = crate::ux_scale::effective_scale();
    let phys = physical_viewport();

    let server = serve_png_once();
    let mut app = markdown_app(&server.url, slot);
    frame(&mut app, phys);

    // Synchronous signal: an image whose fetch has started reports
    // `needs_draw` (Loading, then Ready-but-unseen) until it is painted.
    assert!(
        app.root().needs_draw(),
        "{label}: the on-screen markdown image (logical slot {slot:?}) never \
         started loading after paint"
    );
    server
        .requests
        .recv_timeout(LOAD_TIMEOUT)
        .unwrap_or_else(|_| {
            panic!("{label}: the image server never received a request");
        });

    // Repaint until the decoded image shows, then one more frame so layout
    // has adopted the image's own dimensions (a frame that raced the decode
    // may have drawn it into the placeholder rect).
    let deadline = Instant::now() + LOAD_TIMEOUT;
    while green_bbox(&frame(&mut app, phys)).is_none() {
        assert!(
            Instant::now() < deadline,
            "{label}: the image was fetched but never painted"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let fb = frame(&mut app, phys);
    let (x0, y0, x1, y1) = green_bbox(&fb).expect("image still painted");

    let side = IMG_PX as f64 * scale;
    let (w, h) = ((x1 - x0 + 1) as f64, (y1 - y0 + 1) as f64);
    assert!(
        (w - side).abs() <= 2.0 && (h - side).abs() <= 2.0,
        "{label}: image painted {w}x{h} device px, expected {side}x{side}"
    );
    let md = app.root().children()[0].bounds();
    let (mx0, my0) = (md.x * scale, md.y * scale);
    let (mx1, my1) = ((md.x + md.width) * scale, (md.y + md.height) * scale);
    assert!(
        x0 as f64 >= mx0 - 1.0
            && (x1 + 1) as f64 <= mx1 + 1.0
            && y0 as f64 >= my0 - 1.0
            && (y1 + 1) as f64 <= my1 + 1.0,
        "{label}: image painted at device ({x0},{y0})..({x1},{y1}), outside the \
         markdown's device rect ({mx0},{my0})..({mx1},{my1})"
    );
}

/// The markdown sits in the right/top part of the logical viewport: fully on
/// screen, but at any scale > 1 its device coordinates lie beyond the
/// LOGICAL viewport's width and height.
fn right_top_slot() -> Rect {
    let slot = Rect::new(250.0, 200.0, 150.0, 100.0);
    assert!(slot.x + slot.width <= VP_W && slot.y + slot.height <= VP_H);
    assert!(slot.x * 2.0 > VP_W && slot.y * 2.0 > VP_H);
    slot
}

#[test]
fn right_top_remote_image_loads_at_device_scale_2() {
    let _g = ScaleGuard;
    assert_on_screen_image_loads("device 2 / ux 1", 2.0, 1.0, right_top_slot());
}

#[test]
fn right_top_remote_image_loads_at_device_scale_2_ux_scale_1_5() {
    let _g = ScaleGuard;
    assert_on_screen_image_loads("device 2 / ux 1.5", 2.0, 1.5, right_top_slot());
}

/// Control: at scale 1 logical and device pixels coincide, so this pins the
/// fetch-and-paint path itself.
#[test]
fn right_top_remote_image_loads_at_scale_1() {
    let _g = ScaleGuard;
    assert_on_screen_image_loads("device 1 / ux 1", 1.0, 1.0, right_top_slot());
}

/// Culling still works at HiDPI: an image placed just past the viewport's
/// right edge is never requested.
#[test]
fn off_screen_remote_image_does_not_load_at_device_scale_2() {
    let _g = ScaleGuard;
    crate::set_device_scale(2.0);
    let phys = physical_viewport();

    let server = serve_png_once();
    let mut app = markdown_app(&server.url, Rect::new(VP_W + 10.0, 100.0, 150.0, 100.0));
    let fb = frame(&mut app, phys);

    assert!(
        !app.root().needs_draw(),
        "an off-screen markdown image started loading"
    );
    assert!(
        server
            .requests
            .recv_timeout(Duration::from_millis(500))
            .is_err(),
        "the image server received a request for an off-screen image"
    );
    assert!(green_bbox(&fb).is_none(), "an off-screen image was painted");
}
