//! End-to-end: a widget painting inside an offscreen compositing layer at
//! HiDPI must find itself inside the paint clip when it is fully on screen.
//!
//! `crate::widget::is_local_rect_in_paint_clip` (and the clip stack behind
//! it, see `widget/paint.rs`) converts local rects to root coordinates with
//! `DrawCtx::root_transform`. Self-animating widgets (`Spinner`,
//! `ProgressBar`) gate their animation on it, so a wrong root mapping inside a
//! layer makes a fully visible widget think it is clipped out and freezes it.
//! The pixel-level backend contract is pinned in `root_transform_layers.rs`;
//! this suite drives the real `App` paint path at device scale 2 (alone and
//! combined with a 1.5 UX scale) through a tree of
//! `layer host → Window → ScrollView → probe`.
//!
//! `GfxCtx` has no retained layers, so a `Window`'s FBO backbuffer is not
//! used here and the `Window` paints directly; the enclosing `LayerHost`
//! (a [`Widget::compositing_layer`] widget, as in
//! `widgets/combo_popup.rs::test_combo_popup_uses_root_transform_inside_layer`)
//! is what routes the subtree through `push_layer`. It sits at a non-zero
//! root offset so the layer origin is non-zero.

use super::*;
use crate::draw_ctx::DrawCtx;
use crate::text::Font;
use crate::widget::CompositingLayer;
use crate::widgets::window::Window;
use crate::{Event, EventResult, Rect, Stack};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// Logical viewport.
const VP_W: f64 = 400.0;
const VP_H: f64 = 300.0;

/// Restores the thread-local device and UX scales even if an assertion fails.
struct ScaleGuard;
impl Drop for ScaleGuard {
    fn drop(&mut self) {
        crate::set_device_scale(1.0);
        crate::ux_scale::set_ux_scale(1.0);
    }
}

/// What the probe observed during its most recent paint.
#[derive(Default)]
struct ProbeReport {
    /// `is_local_rect_in_paint_clip` for the probe's whole bounds.
    in_clip: Cell<Option<bool>>,
    /// The probe's centre mapped through `root_transform` (device pixels).
    centre_root: Cell<Option<(f64, f64)>>,
}

/// Fills its bounds with opaque red and records what the paint-clip query
/// and `root_transform` report for it.
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
        ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();
        self.report
            .in_clip
            .set(Some(crate::widget::is_local_rect_in_paint_clip(
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

/// Paints its subtree through an offscreen compositing layer; its single
/// child fills it.
struct LayerHost {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for LayerHost {
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
        let child = &mut self.children[0];
        child.layout(available);
        child.set_bounds(Rect::new(0.0, 0.0, available.width, available.height));
        available
    }
    fn compositing_layer(&mut self) -> Option<CompositingLayer> {
        Some(CompositingLayer::new(0.0, 0.0, 0.0, 0.0, 1.0))
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// App root: fills the viewport and pins its single child at `slot`.
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
        child.layout(Size::new(slot.width, slot.height));
        child.set_bounds(slot);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Lay out and paint `layer host → Window → ScrollView → probe` once at the
/// given scales; return the probe's report and the painted root target.
fn paint_probe_tree(device_scale: f64, ux_scale: f64) -> (Rc<ProbeReport>, Framebuffer) {
    crate::set_device_scale(device_scale);
    crate::ux_scale::set_ux_scale(ux_scale);

    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
    let report = Rc::new(ProbeReport::default());
    let probe = Probe {
        bounds: Rect::default(),
        children: Vec::new(),
        report: Rc::clone(&report),
    };
    let scroll = ScrollView::new(Box::new(probe));
    let win = Window::new("Probe", font, Box::new(scroll))
        .with_bounds(Rect::new(10.0, 10.0, 220.0, 170.0));
    let host = LayerHost {
        bounds: Rect::default(),
        children: vec![Box::new(Stack::new().add(Box::new(win)))],
    };
    // A non-zero slot puts the layer origin away from the root origin — at
    // device scale 1 or origin (0, 0) the composition order cannot matter.
    let root = Placer {
        bounds: Rect::default(),
        children: vec![Box::new(host)],
        slot: Rect::new(150.0, 100.0, 240.0, 190.0),
    };
    let mut app = App::new(Box::new(root));

    let scale = crate::ux_scale::effective_scale();
    let phys = Size::new((VP_W * scale).round(), (VP_H * scale).round());
    app.layout(phys);
    let mut fb = Framebuffer::new(phys.width as u32, phys.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        app.paint(&mut ctx);
    }
    (report, fb)
}

/// Shared assertions: the probe painted, reported itself inside the paint
/// clip, and `root_transform` put its centre on a red (probe) pixel.
fn assert_probe_visible_and_mapped(label: &str, report: &ProbeReport, fb: &Framebuffer) {
    let in_clip = report
        .in_clip
        .get()
        .unwrap_or_else(|| panic!("{label}: the probe never painted"));
    assert!(
        in_clip,
        "{label}: a fully visible probe inside a compositing layer reported \
         itself outside the paint clip (centre mapped to {:?} in a {}x{} target)",
        report.centre_root.get(),
        fb.width(),
        fb.height(),
    );
    let (cx, cy) = report.centre_root.get().expect("probe recorded its centre");
    assert!(
        cx >= 0.0 && cy >= 0.0 && cx < fb.width() as f64 && cy < fb.height() as f64,
        "{label}: root_transform mapped the probe centre to ({cx:.1}, {cy:.1}), \
         outside the {}x{} root target",
        fb.width(),
        fb.height(),
    );
    let px = sample(fb, cx as u32, cy as u32);
    assert!(
        is_red(px),
        "{label}: root_transform mapped the probe centre to ({cx:.1}, {cy:.1}) \
         but the root pixel there is {px:?}, not the probe's red"
    );
}

#[test]
fn probe_inside_layer_is_in_paint_clip_at_device_scale_2() {
    let _g = ScaleGuard;
    let (report, fb) = paint_probe_tree(2.0, 1.0);
    assert_probe_visible_and_mapped("device 2 / ux 1", &report, &fb);
}

#[test]
fn probe_inside_layer_is_in_paint_clip_at_device_scale_2_ux_scale_1_5() {
    let _g = ScaleGuard;
    let (report, fb) = paint_probe_tree(2.0, 1.5);
    assert_probe_visible_and_mapped("device 2 / ux 1.5", &report, &fb);
}

/// Control: at scale 1 the composition order is irrelevant, so this pins
/// that the tree itself (layout, layer, clip stack) is sound.
#[test]
fn probe_inside_layer_is_in_paint_clip_at_scale_1() {
    let _g = ScaleGuard;
    let (report, fb) = paint_probe_tree(1.0, 1.0);
    assert_probe_visible_and_mapped("device 1 / ux 1", &report, &fb);
}
