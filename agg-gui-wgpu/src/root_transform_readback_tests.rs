//! Headless GPU readback tests for `DrawCtx::root_transform` inside offscreen
//! layers on [`WgpuGfxCtx`].
//!
//! The contract: `root_transform()` maps a widget-local point to the ROOT
//! render target's device pixels (Y-up), however many compositing or clip
//! layers the caller is nested in. Layer origins are recorded in the parent's
//! device pixels, so they are added AFTER the in-layer CTM — never fed through
//! its scale. Every scenario runs under a HiDPI-style `scale(2, 2)`, because
//! at scale 1 the two composition orders agree.
//!
//! Each test draws an opaque red rect at a known local position inside the
//! layer(s), captures `root_transform()` there, renders through the real
//! deferred-command pipeline, reads the pixels back, and asserts the captured
//! transform maps the local rect onto where the red actually landed. A final
//! end-to-end test paints an `App` whose `Window` composites through its
//! retained FBO (the production HiDPI path) and checks the paint-clip query
//! that `Spinner` / `ProgressBar` gate their animation on.
//!
//! Software twins: `agg-gui/src/tests/root_transform_layers.rs` and
//! `agg-gui/src/tests/root_transform_paint_clip.rs`. Like the other readback
//! tests, these are skipped (pass trivially) when no GPU adapter is available.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::text::Font;
use agg_gui::{
    App, Event, EventResult, Rect, ScrollView, Size, Stack, TransAffine, Widget, Window,
};

use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::WgpuGfxCtx;

/// Square target for the layer scenarios; a multiple of 64 keeps rows
/// 256-byte aligned for the readback.
const SIZE: u32 = 128;

/// In-crate test font (see `layer_text_readback_tests::TEST_FONT`).
const TEST_FONT: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");

/// Inclusive Y-up pixel bounding box `(min_x, min_y, max_x, max_y)`.
type PixelBox = (u32, u32, u32, u32);

fn is_red(p: [u8; 4]) -> bool {
    p[0] > 200 && p[1] < 60 && p[2] < 60
}

/// Sample a top-row-first readback at Y-up `(x, y_up)`.
fn at(data: &[u8], w: u32, h: u32, x: u32, y_up: u32) -> [u8; 4] {
    px(data, w, x, h - 1 - y_up)
}

/// Y-up bounding box of the red pixels in a top-row-first readback.
fn red_box(data: &[u8], w: u32, h: u32) -> Option<PixelBox> {
    let mut found: Option<PixelBox> = None;
    for y in 0..h {
        for x in 0..w {
            if is_red(at(data, w, h, x, y)) {
                found = Some(match found {
                    None => (x, y, x, y),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                });
            }
        }
    }
    found
}

/// Assert `rt` maps the local rect `(x, y, w, h)` onto the device pixels it
/// actually rasterised to (`found`), within one pixel on every edge.
fn assert_maps_onto_pixels(
    label: &str,
    rt: &TransAffine,
    local: (f64, f64, f64, f64),
    found: Option<PixelBox>,
) {
    let Some((px0, py0, px1, py1)) = found else {
        panic!("{label}: the red probe rect never reached the root target");
    };
    let (x, y, w, h) = local;
    let (mut ax, mut ay) = (x, y);
    let (mut bx, mut by) = (x + w, y + h);
    rt.transform(&mut ax, &mut ay);
    rt.transform(&mut bx, &mut by);
    let mapped = (ax.min(bx), ay.min(by), ax.max(bx), ay.max(by));
    // The pixel box is inclusive; its device-space span ends one past it.
    let actual = (px0 as f64, py0 as f64, (px1 + 1) as f64, (py1 + 1) as f64);
    let close = (mapped.0 - actual.0).abs() <= 1.0
        && (mapped.1 - actual.1).abs() <= 1.0
        && (mapped.2 - actual.2).abs() <= 1.0
        && (mapped.3 - actual.3).abs() <= 1.0;
    assert!(
        close,
        "{label}: root_transform maps local {local:?} to device \
         [{:.1}, {:.1}]-[{:.1}, {:.1}], but the rect landed at device \
         [{}, {}]-[{}, {}] in the root target (rt = {rt:?})",
        mapped.0, mapped.1, mapped.2, mapped.3, actual.0, actual.1, actual.2, actual.3,
    );
}

fn fill_red(ctx: &mut dyn DrawCtx, x: f64, y: f64, w: f64, h: f64) {
    ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 1.0));
    ctx.begin_path();
    ctx.rect(x, y, w, h);
    ctx.fill();
}

/// Run `scenario` on a fresh white `SIZE`² context; return the transform it
/// captured and where red landed, or `None` without a GPU adapter.
fn run(
    scenario: impl FnOnce(&mut dyn DrawCtx) -> TransAffine,
) -> Option<(TransAffine, Option<PixelBox>)> {
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
    ctx.clear(Color::white());
    let rt = scenario(&mut ctx);
    ctx.flush_to_surface(&target.view);
    let data = target.read();
    Some((rt, red_box(&data, SIZE, SIZE)))
}

/// Device scale 2 plus a logical offset puts the layer origin at device
/// (20, 30); inside, the local CTM is `scale(2)` then a further translate.
#[test]
fn push_layer_root_transform_matches_pixels_at_scale_2() {
    let local = (3.0, 4.0, 4.0, 4.0);
    let Some((rt, found)) = run(|ctx| {
        ctx.scale(2.0, 2.0);
        ctx.translate(10.0, 15.0);
        ctx.push_layer(40.0, 40.0);
        ctx.translate(5.0, 7.0);
        let rt = ctx.root_transform();
        fill_red(ctx, local.0, local.1, local.2, local.3);
        ctx.pop_layer();
        rt
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_maps_onto_pixels("wgpu push_layer", &rt, local, found);
}

/// Two nested layers: the inner origin is recorded in the outer layer's
/// pixels, so the root position is the in-layer CTM plus both origins.
#[test]
fn nested_push_layer_root_transform_matches_pixels_at_scale_2() {
    let local = (1.0, 1.0, 5.0, 5.0);
    let Some((rt, found)) = run(|ctx| {
        ctx.scale(2.0, 2.0);
        ctx.translate(10.0, 15.0);
        ctx.push_layer(40.0, 40.0);
        ctx.translate(6.0, 4.0);
        ctx.push_layer(20.0, 20.0);
        ctx.translate(2.0, 3.0);
        let rt = ctx.root_transform();
        fill_red(ctx, local.0, local.1, local.2, local.3);
        ctx.pop_layer();
        ctx.pop_layer();
        rt
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_maps_onto_pixels("wgpu nested push_layer", &rt, local, found);
}

/// A `clip_path` layer keeps the parent CTM shifted by the layer origin; the
/// path's device bounds start at (26, 34), off the CTM's own translation, so
/// the in-layer CTM carries a non-zero (negative) local translation too.
#[test]
fn clip_path_layer_root_transform_matches_pixels_at_scale_2() {
    let local = (5.0, 6.0, 4.0, 4.0);
    let Some((rt, found)) = run(|ctx| {
        ctx.scale(2.0, 2.0);
        ctx.translate(10.0, 15.0);
        ctx.save();
        ctx.begin_path();
        ctx.rect(3.0, 2.0, 30.0, 30.0);
        ctx.clip_path();
        ctx.translate(4.0, 5.0);
        let rt = ctx.root_transform();
        fill_red(ctx, local.0, local.1, local.2, local.3);
        ctx.restore();
        rt
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_maps_onto_pixels("wgpu clip_path layer", &rt, local, found);
}

/// Mixed nesting: a `clip_path` layer inside a `push_layer`.
#[test]
fn clip_path_inside_push_layer_root_transform_matches_pixels_at_scale_2() {
    let local = (1.0, 2.0, 4.0, 4.0);
    let Some((rt, found)) = run(|ctx| {
        ctx.scale(2.0, 2.0);
        ctx.translate(10.0, 15.0);
        ctx.push_layer(40.0, 40.0);
        ctx.translate(3.0, 2.0);
        ctx.save();
        ctx.begin_path();
        ctx.rect(1.0, 1.0, 20.0, 20.0);
        ctx.clip_path();
        ctx.translate(2.0, 2.0);
        let rt = ctx.root_transform();
        fill_red(ctx, local.0, local.1, local.2, local.3);
        ctx.restore();
        ctx.pop_layer();
        rt
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_maps_onto_pixels("wgpu clip_path inside push_layer", &rt, local, found);
}

// ---------------------------------------------------------------------------
// End-to-end: App → Window (retained FBO) → ScrollView → probe
// ---------------------------------------------------------------------------

/// Logical viewport for the end-to-end test (device scale 2 → 512×512).
const VP: f64 = 256.0;

/// Restores the thread-local device scale even if an assertion fails.
struct ScaleGuard;
impl Drop for ScaleGuard {
    fn drop(&mut self) {
        agg_gui::set_device_scale(1.0);
    }
}

/// What the probe observed during its most recent paint.
#[derive(Default)]
struct ProbeReport {
    in_clip: Cell<Option<bool>>,
    centre_root: Cell<Option<(f64, f64)>>,
}

/// Fills its bounds with opaque red and records the paint-clip query and its
/// root-mapped centre.
struct Probe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    report: Rc<ProbeReport>,
}

impl Widget for Probe {
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
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(40.0, 24.0)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let (w, h) = (self.bounds.width, self.bounds.height);
        fill_red(ctx, 0.0, 0.0, w, h);
        self.report
            .in_clip
            .set(Some(agg_gui::widget::is_local_rect_in_paint_clip(
                ctx, 0.0, 0.0, w, h,
            )));
        let (mut cx, mut cy) = (w * 0.5, h * 0.5);
        ctx.root_transform().transform(&mut cx, &mut cy);
        self.report.centre_root.set(Some((cx, cy)));
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// On wgpu a `Window` paints through its retained FBO layer, so a probe in
/// its content reads `root_transform` from inside that layer. The window sits
/// high enough that adding its device-pixel origin a second time (the
/// pre-fix composition) pushes the probe off the top of the viewport.
#[test]
fn probe_in_window_fbo_is_in_paint_clip_at_device_scale_2() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let _g = ScaleGuard;
    agg_gui::set_device_scale(2.0);
    let phys = (VP * 2.0) as u32;

    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
    let report = Rc::new(ProbeReport::default());
    let probe = Probe {
        bounds: Rect::default(),
        children: Vec::new(),
        report: Rc::clone(&report),
    };
    let win = Window::new("Probe", font, Box::new(ScrollView::new(Box::new(probe))))
        .with_bounds(Rect::new(110.0, 120.0, 130.0, 120.0));
    let mut app = App::new(Box::new(Stack::new().add(Box::new(win))));
    app.layout(Size::new(phys as f64, phys as f64));

    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), phys, phys);
    let mut ctx = WgpuGfxCtx::new(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        phys as f32,
        phys as f32,
    );
    ctx.reset(phys as f32, phys as f32);
    assert!(
        ctx.supports_retained_layers(),
        "the Window must take its retained-FBO path for this test to mean anything"
    );
    ctx.clear(Color::white());
    app.paint(&mut ctx);
    // `supports_retained_layers` only says the path is available; the Window
    // must actually have stored its retained FBO this frame, or the probe
    // read `root_transform` outside any layer and this test proves nothing.
    assert!(
        !ctx.retained_layers.is_empty(),
        "the Window painted without compositing through a retained FBO layer"
    );
    ctx.flush_to_surface(&target.view);
    let data = target.read();

    let in_clip = report.in_clip.get().expect("the probe never painted");
    assert!(
        in_clip,
        "a fully visible probe inside a Window FBO at device scale 2 reported \
         itself outside the paint clip (centre mapped to {:?} in a {phys}x{phys} target)",
        report.centre_root.get(),
    );
    let (cx, cy) = report.centre_root.get().expect("probe recorded its centre");
    assert!(
        cx >= 0.0 && cy >= 0.0 && cx < phys as f64 && cy < phys as f64,
        "root_transform mapped the probe centre to ({cx:.1}, {cy:.1}), outside \
         the {phys}x{phys} root target"
    );
    let p = at(&data, phys, phys, cx as u32, cy as u32);
    assert!(
        is_red(p),
        "root_transform mapped the probe centre to ({cx:.1}, {cy:.1}) but the \
         root pixel there is {p:?}, not the probe's red"
    );
}
