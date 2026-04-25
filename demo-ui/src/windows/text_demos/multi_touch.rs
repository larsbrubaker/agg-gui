#![allow(unused_imports)]
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    measure_text_metrics, Button, Checkbox, Color, Container, DragValue, DrawCtx, Event,
    EventResult, FlexColumn, FlexRow, Font, Label, LabelAlign, MouseButton, Point, Rect,
    ScrollView, Separator, Size, SizedBox, TextField, Widget,
};

// ---------------------------------------------------------------------------
// Multi Touch demo
// ---------------------------------------------------------------------------
//
// Port of egui's `multi_touch.rs` demo.  Layout + interaction + the
// decaying-arrow trick all match the original as closely as the
// coordinate-system flip allows.  The big visible difference vs. egui
// is Y-up: egui draws the arrow from (-0.5, 0.5) to (0.5, -0.5) in its
// Y-down normalised space, which reads visually as bottom-left →
// top-right; in our Y-up space that same visual is (-0.5, -0.5) to
// (0.5, 0.5).  Everything else — normalised ±1 canvas with square
// proportions, zoom/rotate/translate accumulators, pressure-driven
// stroke width, and the half-life reset animation — is the same.

thread_local! {
    static RELATIVE_POINTER_GESTURE: Cell<bool> = const { Cell::new(false) };
}

/// Accumulated zoom / rotation / translation state for the arrow.
/// Mirrors the fields on egui's `MultiTouch` struct.
struct MultiTouchView {
    bounds: agg_gui::Rect,
    children: Vec<Box<dyn Widget>>,
    /// Multiplicative zoom; starts at 1.0 and pinch deltas multiply in.
    zoom: f64,
    /// Rotation in radians (Y-up CCW).
    rotation: f64,
    /// Translation in NORMALISED units (i.e. `pixels / scale`), so the
    /// arrow tracks the pinch midpoint regardless of widget size — this
    /// is what egui does via `to_screen.inverse().scale() * delta`.
    translation_x: f64,
    translation_y: f64,
    /// Timestamp of the most recent frame that saw a touch gesture.
    /// The reset animation keys off `(now - last_touch_time)`.
    last_touch_time: Option<web_time::Instant>,
    /// Previous frame's instant — used to derive `dt` for the half-life
    /// decay.  `None` until after the first paint.
    prev_frame_time: Option<web_time::Instant>,
    /// Latest frame's force reading (0.0 when unsupported), used to
    /// thicken the stroke.
    force: f32,
    /// Latest frame's finger count.  Surfaced through the status label.
    num_touches: usize,
}

impl MultiTouchView {
    fn new() -> Self {
        Self {
            bounds: agg_gui::Rect::default(),
            children: Vec::new(),
            zoom: 1.0,
            rotation: 0.0,
            translation_x: 0.0,
            translation_y: 0.0,
            last_touch_time: None,
            prev_frame_time: None,
            force: 0.0,
            num_touches: 0,
        }
    }

    /// Uniform pixels-per-normalised-unit scale, matching egui's
    /// `to_screen.scale()`.  The shorter widget axis maps to ±1.
    fn unit_scale(&self) -> f64 {
        self.bounds.width.min(self.bounds.height) * 0.5
    }

    /// Smoothly drift zoom / rotation / translation back toward identity
    /// once the user lifts their fingers.  Same curve as egui: hold for
    /// 0.5 s, then an exponential half-life decay whose time-constant
    /// itself ramps down over the next 0.5 s.
    fn slowly_reset(&mut self, now: web_time::Instant, dt: f64) -> bool {
        let last = match self.last_touch_time {
            Some(t) => t,
            None => return false,
        };
        let time_since_last = now.duration_since(last).as_secs_f64();
        let delay = 0.5_f64;
        if time_since_last < delay {
            return true; // keep ticking, don't change values yet
        }
        // `remap_clamp(time_since_last, 0.5..=1.0, 1.0..=0.0)` from egui.
        let t = ((time_since_last - delay) / (1.0 - delay)).clamp(0.0, 1.0);
        let half_life = (1.0 - t).powi(4);
        if half_life <= 1e-3 {
            self.zoom = 1.0;
            self.rotation = 0.0;
            self.translation_x = 0.0;
            self.translation_y = 0.0;
            return false;
        }
        // dt is the wall-clock delta between frames.
        let factor = (-(2_f64.ln()) / half_life * dt).exp();
        self.zoom = 1.0 + (self.zoom - 1.0) * factor;
        self.rotation *= factor;
        self.translation_x *= factor;
        self.translation_y *= factor;
        true
    }
}

impl Widget for MultiTouchView {
    fn type_name(&self) -> &'static str {
        "MultiTouchView"
    }
    fn bounds(&self) -> agg_gui::Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: agg_gui::Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: agg_gui::Size) -> agg_gui::Size {
        self.bounds = agg_gui::Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn agg_gui::DrawCtx) {
        let now = web_time::Instant::now();
        let dt = match self.prev_frame_time {
            Some(t) => now.duration_since(t).as_secs_f64().clamp(0.0, 0.25),
            None => 1.0 / 60.0,
        };
        self.prev_frame_time = Some(now);

        // ── Integrate this frame's gesture deltas ────────────────────────
        let scale = self.unit_scale();
        let mut stroke_width = 1.0_f32;
        let had_gesture = if let Some(mt) = agg_gui::current_multi_touch() {
            self.zoom *= mt.zoom_delta as f64;
            self.rotation += mt.rotation_delta as f64;
            // Pan delta comes in widget pixels; store in normalised units
            // so the accumulator is resolution-independent.
            if scale > 0.0 {
                self.translation_x += mt.translation_delta.x / scale;
                self.translation_y += mt.translation_delta.y / scale;
            }
            self.force = mt.force;
            self.num_touches = mt.num_touches;
            self.last_touch_time = Some(now);
            stroke_width += 10.0 * mt.force;
            true
        } else {
            self.num_touches = 0;
            self.force = 0.0;
            self.slowly_reset(now, dt)
        };
        if had_gesture {
            agg_gui::animation::request_tick();
        }

        // ── Canvas background ────────────────────────────────────────────
        let v = ctx.visuals();
        ctx.set_fill_color(v.panel_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();

        // ── Arrow geometry ───────────────────────────────────────────────
        //
        // egui draws from (-0.5, 0.5) to (0.5, -0.5) in Y-down, meaning
        // bottom-left → top-right visually.  In Y-up that's
        // (-0.5, -0.5) → (0.5, 0.5).
        let cx = self.bounds.width * 0.5;
        let cy = self.bounds.height * 0.5;
        let zoom = self.zoom;
        let (sin_r, cos_r) = self.rotation.sin_cos();
        let rot_scale = |vx: f64, vy: f64| -> (f64, f64) {
            (
                zoom * (vx * cos_r - vy * sin_r),
                zoom * (vx * sin_r + vy * cos_r),
            )
        };
        let (tail_ox, tail_oy) = rot_scale(-0.5, -0.5);
        let (dir_x, dir_y) = rot_scale(1.0, 1.0);
        let tail_nx = self.translation_x + tail_ox;
        let tail_ny = self.translation_y + tail_oy;
        let tail_px = cx + tail_nx * scale;
        let tail_py = cy + tail_ny * scale;
        let tip_px = tail_px + dir_x * scale;
        let tip_py = tail_py + dir_y * scale;

        // ── Arrow stroke ─────────────────────────────────────────────────
        let color = v.text_color;
        ctx.set_stroke_color(color);
        ctx.set_line_width(stroke_width as f64);
        ctx.begin_path();
        ctx.move_to(tail_px, tail_py);
        ctx.line_to(tip_px, tip_py);
        ctx.stroke();

        // ── Arrow head (filled triangle at the tip) ──────────────────────
        let head_len = (dir_x * scale).hypot(dir_y * scale) * 0.12;
        let tip_len = (tip_px - tail_px).hypot(tip_py - tail_py);
        if tip_len > 1.0 && head_len > 0.5 {
            let ux = (tip_px - tail_px) / tip_len;
            let uy = (tip_py - tail_py) / tip_len;
            let head_half_angle = 0.45_f64;
            let (sa, ca) = head_half_angle.sin_cos();
            let lx = tip_px - head_len * (ux * ca - uy * sa);
            let ly = tip_py - head_len * (uy * ca + ux * sa);
            let rx = tip_px - head_len * (ux * ca + uy * sa);
            let ry = tip_py - head_len * (uy * ca - ux * sa);
            ctx.set_fill_color(color);
            ctx.begin_path();
            ctx.move_to(tip_px, tip_py);
            ctx.line_to(lx, ly);
            ctx.line_to(rx, ry);
            ctx.close_path();
            ctx.fill();
        }
    }

    fn on_event(&mut self, _event: &agg_gui::Event) -> agg_gui::EventResult {
        // Consume drag events so the host window doesn't move when the
        // user single-finger-drags over the canvas.  Matches the
        // `Sense::drag()` workaround egui uses for the same reason.
        match _event {
            agg_gui::Event::MouseWheel {
                delta_y,
                delta_x,
                modifiers,
                ..
            } => {
                let scale = self.unit_scale();
                if modifiers.ctrl || modifiers.meta {
                    let zoom_delta = (1.0 + *delta_y * 0.002).clamp(0.2, 5.0);
                    self.zoom *= zoom_delta;
                } else if scale > 0.0 {
                    self.translation_x += *delta_x / scale;
                    self.translation_y += *delta_y / scale;
                }
                self.last_touch_time = Some(web_time::Instant::now());
                RELATIVE_POINTER_GESTURE.with(|flag| flag.set(true));
                agg_gui::animation::request_tick();
                agg_gui::EventResult::Consumed
            }
            agg_gui::Event::MouseDown { .. }
            | agg_gui::Event::MouseMove { .. }
            | agg_gui::Event::MouseUp { .. } => agg_gui::EventResult::Consumed,
            _ => agg_gui::EventResult::Ignored,
        }
    }

    fn needs_paint(&self) -> bool {
        true
    }
}

/// Build the Multi Touch demo window content.  Single-finger acts like
/// a mouse; two or more fingers produce pinch / rotate / pan gestures
/// that drive the rendered arrow.  Pressure (when the platform reports
/// it) thickens the stroke.
pub fn multi_touch(font: Arc<Font>) -> Box<dyn Widget> {
    let status_font = Arc::clone(&font);

    /// Live status label that re-reads `current_multi_touch` every
    /// layout and formats its text.  Matches egui's "Input source" line.
    struct StatusLabel {
        bounds: agg_gui::Rect,
        children: Vec<Box<dyn Widget>>,
        inner: Label,
    }
    impl Widget for StatusLabel {
        fn type_name(&self) -> &'static str {
            "MultiTouchStatus"
        }
        fn bounds(&self) -> agg_gui::Rect {
            self.bounds
        }
        fn set_bounds(&mut self, b: agg_gui::Rect) {
            self.bounds = b;
            self.inner.set_bounds(b);
        }
        fn children(&self) -> &[Box<dyn Widget>] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            &mut self.children
        }
        fn layout(&mut self, available: agg_gui::Size) -> agg_gui::Size {
            let txt = match agg_gui::current_multi_touch() {
                Some(mt) => format!("Input source: {}-finger touch", mt.num_touches,),
                None => {
                    let cursor = RELATIVE_POINTER_GESTURE.with(|flag| flag.get());
                    if cursor {
                        "Input source: cursor".to_string()
                    } else {
                        "Input source: none".to_string()
                    }
                }
            };
            self.inner.set_text(&txt);
            self.inner.layout(available)
        }
        fn paint(&mut self, ctx: &mut dyn agg_gui::DrawCtx) {
            self.inner.paint(ctx);
        }
        fn on_event(&mut self, _e: &agg_gui::Event) -> agg_gui::EventResult {
            agg_gui::EventResult::Ignored
        }
        fn needs_paint(&self) -> bool {
            true
        }
    }

    let status_label: Box<dyn Widget> = Box::new(StatusLabel {
        bounds: agg_gui::Rect::default(),
        children: Vec::new(),
        inner: Label::new(" ", Arc::clone(&status_font))
            .with_font_size(12.0)
            .with_wrap(true),
    });

    let heading = Label::new(
        "This demo only works on devices with multitouch support \
         (e.g. mobiles, tablets, and trackpads).",
        Arc::clone(&font),
    )
    .with_font_size(13.0)
    .with_wrap(true);

    let hint = Label::new(
        "Try touch gestures Pinch/Stretch, Rotation, and Pressure with 2+ fingers.",
        Arc::clone(&font),
    )
    .with_font_size(11.0)
    .with_wrap(true);

    let view: Box<dyn Widget> = Box::new(MultiTouchView::new());

    let col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(10.0)
        .with_panel_bg()
        .add(Box::new(heading))
        .add(Box::new(Separator::horizontal()))
        .add(Box::new(hint))
        .add(status_label)
        .add_flex(view, 1.0);

    Box::new(col)
}
