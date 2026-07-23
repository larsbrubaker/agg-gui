//! Lion demo — renders the classic AGG lion via the halo-AA pipeline.
//!
//! # Proof of halo-AA correctness + tess2 numerical stability
//!
//! The lion's ~130 coloured polygons go through the full path every frame:
//! the raw polygon coords are rotated / scaled / skewed, fed to
//! `tessellate_path_aa` (which runs tess2 fresh each frame), and the
//! resulting triangles + edge-flag halo strips are submitted to the AA
//! solid shader.  MSAA is explicitly **disabled** on the GL context (no
//! `with_multisampling` in `demo-native/src/main.rs`), so every smooth
//! silhouette pixel you see is coming from the halo strips — analytic
//! edge-coverage, not hardware supersampling.
//!
//! Per-frame re-tessellation is the way libtess2 was designed to be used
//! — that's SGI's whole point, numerically-stable triangulation across
//! arbitrary transforms.  The `tess2-rust` rotation-stability tests
//! (`tests/lion_polygons.rs`) pin that down so dragging never flips the
//! polygon topology.
//!
//! # Interaction
//!
//!   - **Left-drag or middle-drag** (a one-finger touch drag arrives as a
//!     synthetic middle-drag — see `agg_gui::touch_emulation`): rotate +
//!     scale about the widget centre, **relative
//!     to the mouse-down point**.  Unlike the C++ `lion.cpp` reference,
//!     which snaps the lion's angle / scale to the raw cursor vector on
//!     every event, we record the cursor position (and the current
//!     angle / scale) on `MouseDown` and then apply deltas from there
//!     so the lion doesn't jump when the gesture starts.
//!   - **Right-drag**: skew (skew_x = cursor.x, skew_y = cursor.y, divided
//!     by 1000 before entering the affine).  Matches `lion.cpp`.
//!   - **Alpha slider** above the lion.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::layout_props::{HAnchor, VAnchor, WidgetBase};
use agg_gui::{
    Color, DrawCtx, Event, EventResult, Font, Label, MouseButton, Point, Rect, Size, Slider, Widget,
};

// ── Path data ────────────────────────────────────────────────────────────────

/// One coloured sub-polygon in local (mirrored) lion coords.
#[derive(Clone)]
struct LionPath {
    verts: Vec<[f64; 2]>,
    color: Color,
}

/// Parse the AGG lion data blob into a list of coloured sub-paths.
///
/// `lion.txt` is SVG-style Y-down with a horizontal mirror implicitly
/// performed by the C demo's `rotate(angle + PI)` — we bake both transforms
/// into the parsed coordinates so the rest of the widget can treat the data
/// as straightforward Y-up local coords.
fn parse_lion() -> (Vec<LionPath>, (f64, f64, f64, f64)) {
    const DATA: &str = include_str!("lion.txt");
    let mut out: Vec<LionPath> = Vec::new();
    let mut cur_color = Color::black();
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for raw in DATA.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        if line.len() == 6 && line.chars().all(|c| c.is_ascii_hexdigit()) {
            let v = u32::from_str_radix(line, 16).unwrap_or(0);
            let r = ((v >> 16) & 0xFF) as f32 / 255.0;
            let g = ((v >> 8) & 0xFF) as f32 / 255.0;
            let b = (v & 0xFF) as f32 / 255.0;
            cur_color = Color::rgb(r, g, b);
            continue;
        }

        if line.starts_with('M') {
            let mut verts: Vec<[f64; 2]> = Vec::new();
            for tok in line.split_whitespace() {
                if tok == "M" || tok == "L" {
                    continue;
                }
                if let Some((x, y)) = parse_coord(tok) {
                    verts.push([x, y]);
                    if x < min_x {
                        min_x = x;
                    }
                    if y < min_y {
                        min_y = y;
                    }
                    if x > max_x {
                        max_x = x;
                    }
                    if y > max_y {
                        max_y = y;
                    }
                }
            }
            if verts.len() >= 3 {
                out.push(LionPath {
                    verts,
                    color: cur_color,
                });
            }
        }
    }

    // Horizontal mirror + Y-up flip (mirror about the bounding-box midpoint).
    let mid_x = (min_x + max_x) * 0.5;
    let mid_y = (min_y + max_y) * 0.5;
    for p in &mut out {
        for v in &mut p.verts {
            v[0] = 2.0 * mid_x - v[0];
            v[1] = 2.0 * mid_y - v[1];
        }
    }

    (out, (min_x, min_y, max_x, max_y))
}

fn parse_coord(s: &str) -> Option<(f64, f64)> {
    let mut it = s.split(',');
    let x: f64 = it.next()?.parse().ok()?;
    let y: f64 = it.next()?.parse().ok()?;
    Some((x, y))
}

// ── Widget ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    None,
    Rotate,
    Skew,
}

struct LionView {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,

    paths: Vec<LionPath>,
    bbox: (f64, f64, f64, f64),

    angle: f64,
    mouse_scale: f64,
    skew_x: f64,
    skew_y: f64,
    /// Pan offset accumulated from two-finger drag (`translation_delta`),
    /// in widget-local Y-up pixels.  Applied at the very end of the vertex
    /// transform so the whole lion slides under the fingers.  Both the
    /// framework and the gesture aggregate are Y-up, so the vector adds
    /// directly with no flip.
    offset_x: f64,
    offset_y: f64,
    alpha: Rc<Cell<f64>>,
    drag: Drag,
    /// Grip state captured on `MouseDown` for the active left-drag
    /// rotate/scale gesture.  `None` while no left-drag is in flight.
    /// See `apply_rotate` for how each field feeds into the delta math.
    rotate_grip: Option<RotateGrip>,
}

/// Snapshot of the state at the instant a left-drag started — used so
/// rotation / scale deltas accumulate from where the lion already was,
/// rather than snapping to the raw cursor vector on every event.
#[derive(Copy, Clone)]
struct RotateGrip {
    /// Angle (rad) from the widget centre to the mouse-down point.
    grip_polar_angle: f64,
    /// Distance from the widget centre to the mouse-down point.
    grip_polar_dist: f64,
    /// `angle` at the moment the gesture began.
    start_angle: f64,
    /// `mouse_scale` at the moment the gesture began.
    start_scale: f64,
}

impl LionView {
    fn new(alpha: Rc<Cell<f64>>) -> Self {
        let (paths, bbox) = parse_lion();
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            paths,
            bbox,
            angle: 0.0,
            mouse_scale: 1.0,
            skew_x: 0.0,
            skew_y: 0.0,
            offset_x: 0.0,
            offset_y: 0.0,
            alpha,
            drag: Drag::None,
            rotate_grip: None,
        }
    }

    fn fit_scale(&self, w: f64, h: f64) -> f64 {
        let (min_x, min_y, max_x, max_y) = self.bbox;
        let lw = (max_x - min_x).max(1e-6);
        let lh = (max_y - min_y).max(1e-6);
        let pad = 10.0;
        let sx = (w - pad * 2.0) / lw;
        let sy = (h - pad * 2.0) / lh;
        sx.min(sy).max(0.01)
    }

    /// Capture the gesture starting point on `MouseDown`.  The polar
    /// coords of the click relative to the widget centre are stored
    /// alongside the current angle / scale; subsequent drags compute
    /// deltas against these anchors.
    fn begin_rotate_grip(&mut self, pos: Point) {
        let cx = self.bounds.width * 0.5;
        let cy = self.bounds.height * 0.5;
        let dx = pos.x - cx;
        let dy = pos.y - cy;
        self.rotate_grip = Some(RotateGrip {
            grip_polar_angle: dy.atan2(dx),
            grip_polar_dist: (dx * dx + dy * dy).sqrt(),
            start_angle: self.angle,
            start_scale: self.mouse_scale,
        });
    }

    fn apply_rotate(&mut self, pos: Point) {
        let Some(grip) = self.rotate_grip else {
            return;
        };
        let cx = self.bounds.width * 0.5;
        let cy = self.bounds.height * 0.5;
        let dx = pos.x - cx;
        let dy = pos.y - cy;
        let cur_angle = dy.atan2(dx);
        let cur_dist = (dx * dx + dy * dy).sqrt();

        // Rotation: additive delta from the grip's polar angle.
        self.angle = grip.start_angle + (cur_angle - grip.grip_polar_angle);

        // Scale: multiplicative ratio of current / grip distance.  Guard
        // a near-zero grip distance (click landed on the centre) so a
        // tiny denominator doesn't explode the scale; in that case just
        // leave the scale where it was.
        if grip.grip_polar_dist > 1e-3 {
            self.mouse_scale = grip.start_scale * (cur_dist / grip.grip_polar_dist);
        }
    }

    fn apply_skew(&mut self, pos: Point) {
        self.skew_x = pos.x;
        self.skew_y = pos.y;
    }

    /// Fold one frame's multi-touch gesture aggregate into the lion's
    /// angle / scale / pan.  Driven from `on_event` now that the gesture is
    /// a routed, captured [`Event::MultiTouch`] (see
    /// `agg_gui::widget::app::gesture`) rather than a paint-time global read.
    ///
    /// We consume zoom / rotation / translation from the aggregate instead
    /// of the single-finger grip math, and invalidate the grip so the
    /// mouse-up path doesn't re-anchor to a stale snapshot.  Per-frame
    /// deltas telescope: the product of every frame's `zoom_delta` is the
    /// spread ratio since the gesture began, the sum of every frame's
    /// `rotation_delta` is the total twist, and the sum of every
    /// `translation_delta` is the total pan.  Consuming the event marks
    /// this window's cached FBO subtree dirty through the framework's
    /// `Consumed → request_draw → mark_dirty` chain, so the re-raster and
    /// the next frame both flow with no gesture-specific `needs_draw` latch.
    fn fold_multi_touch(&mut self, mt: &agg_gui::MultiTouchInfo) {
        self.mouse_scale = (self.mouse_scale * mt.zoom_delta as f64).clamp(0.05, 50.0);
        self.angle += mt.rotation_delta as f64;
        self.offset_x += mt.translation_delta.x;
        self.offset_y += mt.translation_delta.y;
        self.rotate_grip = None;
    }
}

impl Widget for LionView {
    fn type_name(&self) -> &'static str {
        "LionView"
    }
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

    fn margin(&self) -> agg_gui::Insets {
        self.base.margin
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn min_size(&self) -> Size {
        self.base.min_size
    }
    fn max_size(&self) -> Size {
        self.base.max_size
    }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        // The multi-touch fold now happens in `on_event` (routed, captured
        // `Event::MultiTouch`).  Consuming that event marks this window's
        // cached FBO subtree dirty via the framework's standard
        // `Consumed → request_draw → mark_dirty` chain, so no gesture-gated
        // `needs_draw` latch is needed to force the re-raster — the default
        // (quiescent when idle) is correct.
        let w = self.bounds.width;
        let h = self.bounds.height;
        if w < 4.0 || h < 4.0 {
            return;
        }

        // Background card.
        let v = ctx.visuals();
        ctx.set_fill_color(v.panel_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        let (min_x, min_y, max_x, max_y) = self.bbox;
        let cx_lion = (min_x + max_x) * 0.5;
        let cy_lion = (min_y + max_y) * 0.5;
        let cx_widget = w * 0.5;
        let cy_widget = h * 0.5;

        let base_scale = self.fit_scale(w, h);
        let scale = base_scale * self.mouse_scale;
        let (sin_a, cos_a) = self.angle.sin_cos();
        let skew_x = self.skew_x / 1000.0;
        let skew_y = self.skew_y / 1000.0;
        let alpha = self.alpha.get().clamp(0.0, 1.0);
        ctx.set_global_alpha(alpha);

        // Fresh tessellation every frame: emit each polygon through the
        // path API, let `do_fill` route it through `tessellate_path_aa`.
        // This is the load tess2 was designed for — running on rotated
        // floats every frame and producing topologically identical output.
        for path in &self.paths {
            ctx.set_fill_color(path.color);
            ctx.begin_path();
            let mut first = true;
            for &[x0, y0] in &path.verts {
                let px = (x0 - cx_lion) * scale;
                let py = (y0 - cy_lion) * scale;
                let rx = px * cos_a - py * sin_a;
                let ry = px * sin_a + py * cos_a;
                let sx = rx + ry * skew_x;
                let sy = ry + rx * skew_y;
                // Two-finger pan is applied last, after rotate + skew: both
                // the framework and the gesture aggregate are Y-up, so the
                // offset vector adds directly.
                let fx = sx + cx_widget + self.offset_x;
                let fy = sy + cy_widget + self.offset_y;
                if first {
                    ctx.move_to(fx, fy);
                    first = false;
                } else {
                    ctx.line_to(fx, fy);
                }
            }
            ctx.close_path();
            ctx.fill();
        }

        ctx.set_global_alpha(1.0);
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            // Two-finger gesture: the framework routes this here (captured to
            // the widget the gesture started over), replacing the old
            // paint-time global read.  Fold zoom / rotation / pan and consume.
            Event::MultiTouch { info } => {
                self.fold_multi_touch(info);
                EventResult::Consumed
            }
            Event::MouseDown { button, pos, .. } => {
                match button {
                    // Middle is accepted alongside Left because the
                    // touch shells synthesize a finger drag as a
                    // middle-button drag — consuming it here means one
                    // finger rotates the lion instead of scrolling the
                    // enclosing window.
                    MouseButton::Left | MouseButton::Middle => {
                        self.drag = Drag::Rotate;
                        // Capture the grip point so subsequent moves
                        // translate into deltas from here — no snap.
                        self.begin_rotate_grip(*pos);
                    }
                    MouseButton::Right => {
                        self.drag = Drag::Skew;
                        self.apply_skew(*pos);
                    }
                    _ => return EventResult::Ignored,
                }
                agg_gui::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseMove { pos } => {
                // Suppress single-finger rotate while a multi-touch
                // gesture is in flight: the first finger still fires
                // MouseMove events (we emulate it as the mouse cursor),
                // but the real zoom/rotation is driven by the gesture
                // aggregate.  The skew branch has no multi-touch
                // analogue, so it stays active.
                match self.drag {
                    Drag::Rotate if agg_gui::current_multi_touch().is_none() => {
                        self.apply_rotate(*pos)
                    }
                    Drag::Rotate => {}
                    Drag::Skew => {
                        self.apply_skew(*pos);
                    }
                    Drag::None => return EventResult::Ignored,
                }
                agg_gui::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseUp { .. } => {
                let was = self.drag != Drag::None;
                self.drag = Drag::None;
                self.rotate_grip = None;
                if was {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseWheel { pos, delta_y, .. } => {
                // Exponential zoom: each wheel notch multiplies scale by
                // a fixed factor so zoom-in and zoom-out are symmetric
                // and never cross zero.  Positive `delta_y` = wheel down
                // in agg-gui's convention; treat that as zoom-out.
                let factor = (-delta_y * 0.1).exp();
                self.mouse_scale = (self.mouse_scale * factor).clamp(0.05, 50.0);
                // If the user is mid-drag, fold the new scale into the
                // grip's `start_scale` so the next `apply_rotate` doesn't
                // undo this wheel input on the very next move event.
                if let Some(grip) = self.rotate_grip.as_mut() {
                    grip.start_scale = self.mouse_scale;
                    let cx = self.bounds.width * 0.5;
                    let cy = self.bounds.height * 0.5;
                    let dx = pos.x - cx;
                    let dy = pos.y - cy;
                    grip.grip_polar_dist = (dx * dx + dy * dy).sqrt();
                    grip.grip_polar_angle = dy.atan2(dx);
                    grip.start_angle = self.angle;
                }
                agg_gui::animation::request_draw();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agg_gui::{Modifiers, MultiTouchInfo, TouchDeviceId};

    fn view() -> LionView {
        let mut v = LionView::new(Rc::new(Cell::new(1.0)));
        v.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
        v
    }

    fn drag(v: &mut LionView, button: MouseButton) -> (EventResult, EventResult) {
        // Down off-centre, then move to a different polar angle around
        // the widget centre (200, 150) so a rotate must change `angle`.
        let down = v.on_event(&Event::MouseDown {
            pos: Point::new(300.0, 150.0),
            button,
            modifiers: Modifiers::default(),
        });
        let mv = v.on_event(&Event::MouseMove {
            pos: Point::new(300.0, 250.0),
        });
        (down, mv)
    }

    /// The touch shells synthesize a finger drag as a MIDDLE-button
    /// drag — the lion must treat it as rotate/scale or one-finger
    /// touch does nothing (the drag falls through to ScrollView).
    #[test]
    fn middle_drag_rotates() {
        let mut v = view();
        let before = v.angle;
        let (down, mv) = drag(&mut v, MouseButton::Middle);
        assert_eq!(down, EventResult::Consumed);
        assert_eq!(mv, EventResult::Consumed);
        assert_ne!(v.angle, before, "middle-drag must rotate the lion");
    }

    /// The gesture is now a routed, captured `Event::MultiTouch` delivered
    /// through `on_event` (no more paint-time global read / `needs_draw`
    /// latch).  Folding one aggregate must multiply scale by `zoom_delta`,
    /// add `rotation_delta` to the angle, accumulate `translation_delta`
    /// into the pan offset, and consume the event so the framework marks
    /// the cached window subtree dirty and re-rasters.
    #[test]
    fn multi_touch_event_folds_zoom_rotation_and_pan() {
        let mut v = view();
        let angle0 = v.angle;
        let scale0 = v.mouse_scale;

        let result = v.on_event(&Event::MultiTouch {
            info: MultiTouchInfo {
                device_id: TouchDeviceId(0),
                num_touches: 2,
                zoom_delta: 1.5,
                rotation_delta: 0.3,
                translation_delta: Point::new(10.0, 5.0),
                force: 0.0,
                center_pos: Point::new(200.0, 150.0),
            },
        });

        assert_eq!(
            result,
            EventResult::Consumed,
            "consuming the gesture is what marks the cached window subtree dirty"
        );
        // Tolerance absorbs the `f32` deltas widening to `f64` in the fold
        // (0.3_f32 as f64 == 0.30000001…), not any behavioural slack.
        assert!(
            (v.angle - (angle0 + 0.3)).abs() < 1e-6,
            "rotation_delta must add to angle: {} vs {}",
            v.angle,
            angle0 + 0.3
        );
        assert!(
            (v.mouse_scale - scale0 * 1.5).abs() < 1e-6,
            "zoom_delta must multiply mouse_scale: {} vs {}",
            v.mouse_scale,
            scale0 * 1.5
        );
        assert!(
            (v.offset_x - 10.0).abs() < 1e-6 && (v.offset_y - 5.0).abs() < 1e-6,
            "translation_delta must accumulate into the pan offset: ({},{})",
            v.offset_x,
            v.offset_y
        );
    }

    /// Regression guard: the classic left-drag rotate still works.
    #[test]
    fn left_drag_rotates() {
        let mut v = view();
        let before = v.angle;
        let (down, mv) = drag(&mut v, MouseButton::Left);
        assert_eq!(down, EventResult::Consumed);
        assert_eq!(mv, EventResult::Consumed);
        assert_ne!(v.angle, before, "left-drag must rotate the lion");
    }
}

// ── Demo window entry point ──────────────────────────────────────────────────

pub fn lion_demo(font: Arc<Font>) -> Box<dyn Widget> {
    use agg_gui::FlexColumn;

    let alpha = Rc::new(Cell::new(1.0f64));
    let alp_c = Rc::clone(&alpha);
    let alp_slider = Slider::new(1.0, 0.0, 1.0, Arc::clone(&font)).on_change(move |v| alp_c.set(v));

    let alp_label = Label::new("Alpha", Arc::clone(&font)).with_font_size(12.0);
    let note = Label::new(
        "Left-drag or one-finger drag: rotate + scale (relative to \
         start).  Wheel / pinch: zoom.  Two-finger twist: rotate.  \
         Two-finger drag: pan.  Right-drag: skew.  MSAA is off; smooth \
         silhouette = halo-AA edges; fresh tess2 every frame.",
        Arc::clone(&font),
    )
    .with_font_size(11.0)
    .with_wrap(true);

    let view = LionView::new(alpha);

    Box::new(
        FlexColumn::new()
            .with_gap(6.0)
            .with_padding(8.0)
            .add(Box::new(alp_label))
            .add(Box::new(alp_slider))
            .add(Box::new(note))
            .add_flex(Box::new(view), 1.0),
    )
}
