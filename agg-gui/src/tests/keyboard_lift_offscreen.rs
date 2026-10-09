//! `logical_root_transform` inside offscreen paint targets while the
//! on-screen keyboard lifts the tree.
//!
//! The lift contract (see `keyboard_lift_harness.rs`): root logical space is
//! the UNLIFTED layout space, and `logical_root_transform(ctx)` takes App's
//! keyboard-lift translate back out of `root_transform` (it knows the
//! translate through `keyboard_scroll::paint_lift`). That is only right where
//! `root_transform` actually carries the lift:
//!
//! * a **CPU backbuffer** (`backbuffer_cache_mut`, `paint/offscreen.rs`)
//!   rasters into a FRESH `GfxCtx` / `LcdGfxCtx` whose `root_transform` is
//!   relative to the bitmap and never saw the lift, so the paint lift must
//!   not be subtracted there;
//! * a **compositing layer** pushes onto the App's own ctx, whose
//!   `root_transform` adds the layer origin — which includes the lift — so it
//!   must still be subtracted (guard).
//!
//! Each case lays a probe out at a known root position, paints the real `App`
//! under a pinned lift, and checks the origin the probe saw.

use super::keyboard_lift_harness::{near, paint_frame, LiftGuard, Place, ScaleGuard};
use super::*;

use crate::geometry::{Point, Rect};
use crate::widget::{BackbufferCache, BackbufferMode, CompositingLayer};
use crate::{DrawCtx, Event, EventResult};
use std::cell::Cell;
use std::rc::Rc;

const VP: Size = Size {
    width: 300.0,
    height: 200.0,
};
const L: f64 = 60.0;
/// The probe's slot inside its offscreen container, which itself sits at the
/// root origin — so this is also the probe's root layout position.
const PROBE: Rect = Rect::new(40.0, 50.0, 30.0, 20.0);

/// What the probe saw while painting.
#[derive(Clone, Copy, Debug, Default)]
struct Seen {
    /// Its origin through `logical_root_transform`.
    logical: Point,
    /// Translation of the plain CTM and of `root_transform` — different
    /// translations prove the probe painted inside a layer.
    ctm: (f64, f64),
    root: (f64, f64),
}

/// Records [`Seen`] every paint.
struct Probe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    seen: Rc<Cell<Option<Seen>>>,
}

impl Widget for Probe {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let (mut x, mut y) = (0.0, 0.0);
        crate::widget::logical_root_transform(ctx).transform(&mut x, &mut y);
        let (t, r) = (ctx.transform(), ctx.root_transform());
        self.seen.set(Some(Seen {
            logical: Point::new(x, y),
            ctm: (t.tx, t.ty),
            root: (r.tx, r.ty),
        }));
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// How the container paints its subtree.
#[derive(Clone, Copy, Debug)]
enum Offscreen {
    /// CPU backbuffer in the given mode (a fresh sub ctx).
    Cpu(BackbufferMode),
    /// A compositing layer on the App's own ctx.
    Layer,
}

/// Container that places the probe at [`PROBE`] and paints through `kind`.
struct Container {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    cache: BackbufferCache,
    kind: Offscreen,
}

impl Widget for Container {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        for child in &mut self.children {
            child.layout(Size::new(PROBE.width, PROBE.height));
            child.set_bounds(PROBE);
        }
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        // Opaque, so the LCD-coverage backbuffer's contract holds.
        ctx.set_fill_color(Color::rgba(0.2, 0.2, 0.2, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
        match self.kind {
            Offscreen::Cpu(_) => Some(&mut self.cache),
            Offscreen::Layer => None,
        }
    }
    fn backbuffer_mode(&self) -> BackbufferMode {
        match self.kind {
            Offscreen::Cpu(mode) => mode,
            Offscreen::Layer => BackbufferMode::Rgba,
        }
    }
    fn compositing_layer(&mut self) -> Option<CompositingLayer> {
        match self.kind {
            Offscreen::Cpu(_) => None,
            Offscreen::Layer => Some(CompositingLayer::new(0.0, 0.0, 0.0, 0.0, 1.0)),
        }
    }
}

/// Paint an app whose root places a `kind` container at the root origin
/// (holding the probe at [`PROBE`]) under `lift`, and return what the probe saw.
fn probe_under_lift(kind: Offscreen, lift: f64) -> Seen {
    let _scales = ScaleGuard::set(1.0, 1.0);
    let _lift = LiftGuard::set(lift);
    let seen = Rc::new(Cell::new(None));
    let probe = Probe {
        bounds: Rect::default(),
        children: Vec::new(),
        seen: Rc::clone(&seen),
    };
    let container = Container {
        bounds: Rect::default(),
        children: vec![Box::new(probe)],
        cache: BackbufferCache::new(),
        kind,
    };
    let mut app = App::new(Box::new(
        Place::new().at(Rect::new(0.0, 0.0, 200.0, 150.0), Box::new(container)),
    ));
    app.layout(VP);
    let _ = paint_frame(&mut app, VP, Color::black());
    seen.get().expect("the probe must paint")
}

/// A probe inside a CPU backbuffer (`kind`) whose container sits at the root
/// origin must see its root layout position (40, 50) under lift, the same as
/// at lift 0. The sub ctx's `root_transform` is bitmap-relative and never had
/// the lift, so subtracting the paint lift there puts it at (40, −10).
fn assert_cpu_backbuffer_probe_sees_root_position(mode: BackbufferMode) {
    let at_rest = probe_under_lift(Offscreen::Cpu(mode), 0.0);
    let lifted = probe_under_lift(Offscreen::Cpu(mode), L);
    let want = Point::new(PROBE.x, PROBE.y);
    let close = |p: Point| near(p.x, want.x, 1e-6) && near(p.y, want.y, 1e-6);
    assert!(
        close(at_rest.logical),
        "precondition ({mode:?}): at lift 0 a probe in a CPU backbuffer at the root \
         origin sees its root layout position {want:?}; got {:?}",
        at_rest.logical
    );
    assert!(
        close(lifted.logical),
        "under lift {L} a probe inside a {mode:?} CPU backbuffer must still see its \
         root layout position {want:?} through logical_root_transform (the fresh \
         sub ctx never carried the lift); got {:?}",
        lifted.logical
    );
}

#[test]
fn lifted_cpu_rgba_backbuffer_probe_sees_its_root_position() {
    assert_cpu_backbuffer_probe_sees_root_position(BackbufferMode::Rgba);
}

#[test]
fn lifted_cpu_lcd_backbuffer_probe_sees_its_root_position() {
    assert_cpu_backbuffer_probe_sees_root_position(BackbufferMode::LcdCoverage);
}

/// GUARD. A probe inside a compositing layer on the software `GfxCtx` paints
/// on the App's own ctx: `root_transform` adds the layer origin, which
/// includes the lift, so `logical_root_transform` must still take it out and
/// the probe sees its root layout position (40, 50).
#[test]
fn lifted_compositing_layer_probe_sees_its_root_position() {
    let seen = probe_under_lift(Offscreen::Layer, L);
    assert!(
        !near(seen.ctm.1, seen.root.1, 1e-6),
        "precondition: the probe must paint inside a compositing layer (its CTM \
         translation {:?} differs from its root_transform translation {:?})",
        seen.ctm,
        seen.root
    );
    let want = Point::new(PROBE.x, PROBE.y);
    assert!(
        near(seen.logical.x, want.x, 1e-6) && near(seen.logical.y, want.y, 1e-6),
        "under lift {L} a probe inside a compositing layer must see its root layout \
         position {want:?} through logical_root_transform; got {:?} (seen {seen:?})",
        seen.logical
    );
}
