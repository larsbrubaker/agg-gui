//! The "Dancing Strings" animation demo — three animated standing-wave
//! harmonics (modes 2, 3, 5) matching egui's `DancingStrings`.
//!
//! Part of the [`super`] animation demo group.  See the module-level notes
//! below for the exact per-point coordinate math ported from egui.

use std::sync::Arc;

use agg_gui::{
    Color, DrawCtx, Event, EventResult, FlexColumn, Font, Point, Rect, Size, Widget,
};

// ---------------------------------------------------------------------------
// DancingStrings — matches egui's `DancingStrings` demo
// ---------------------------------------------------------------------------
//
// egui draws three standing-wave harmonics with modes 2, 3, 5.  For each
// point index `i` in 0..=N the coordinates are:
//
//     t    = i / N                     // 0..1 normalised x
//     amp  = sin(time · speed · mode) / mode
//     y    = amp · sin(t · π · mode)   // −1..1 normalised y
//
// Then `(t, y)` is mapped from x_range 0..1, y_range −1..1 onto the canvas
// rect.  Line thickness per mode is `10 / mode` (so mode 2 is thickest,
// mode 5 thinnest).  Color is a single high-alpha text-like tone; the
// optional "Colored" toggle renders a center-teal → edges-pink gradient
// along the path (trans-flag colors).

/// Animated sine-wave display (egui parity).
struct DancingStrings {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    start: web_time::Instant,
    colored: std::rc::Rc<std::cell::Cell<bool>>,
}

impl DancingStrings {
    fn new(colored: std::rc::Rc<std::cell::Cell<bool>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            start: web_time::Instant::now(),
            colored,
        }
    }
}

impl Widget for DancingStrings {
    fn type_name(&self) -> &'static str {
        "DancingStrings"
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

    /// Continuous sine-wave animation — every frame samples
    /// `self.start.elapsed()` so the paint output changes every tick.
    /// Returning `true` from the tree walk keeps the host rendering.  The
    /// visibility gate short-circuits at the enclosing Window when the
    /// Dancing Strings demo is closed, so this doesn't keep the loop
    /// running when the widget isn't on screen.
    fn needs_draw(&self) -> bool {
        true
    }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        use std::f64::consts::PI;
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let time = self.start.elapsed().as_secs_f64();
        let colored = self.colored.get();

        // Canvas background (egui uses Frame::canvas which draws a subtle
        // tinted rect + border).
        ctx.set_fill_color(v.bg_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.stroke();

        // Base color — dark theme: luminous white α=196/255; light: black α=240/255.
        let base = if v.bg_color.r + v.bg_color.g + v.bg_color.b < 1.0 {
            Color::rgba(1.0, 1.0, 1.0, 196.0 / 255.0)
        } else {
            Color::rgba(0.0, 0.0, 0.0, 240.0 / 255.0)
        };
        // Trans-flag gradient endpoints (egui hex_colors).
        let center_color = Color::rgb(
            0x5B as f32 / 255.0,
            0xCE as f32 / 255.0,
            0xFA as f32 / 255.0,
        );
        let outer_color = Color::rgb(
            0xF5 as f32 / 255.0,
            0xA9 as f32 / 255.0,
            0xB8 as f32 / 255.0,
        );

        let speed = 1.5_f64;
        let n = 120_usize;

        for &mode_i in &[2_u32, 3, 5] {
            let mode = mode_i as f64;
            let thickness = 10.0 / mode;

            // In "colored" mode, draw each segment with its own interpolated
            // color so the full path shows the gradient.  Otherwise draw the
            // path as one stroked polyline in the base color.
            if colored {
                // Iterate segments, interpolating color based on segment midpoint.
                ctx.set_line_width(thickness);
                let mut prev: Option<(f64, f64)> = None;
                for i in 0..=n {
                    let t = i as f64 / n as f64;
                    let amp = (time * speed * mode).sin() / mode;
                    let y_n = amp * (t * PI * mode).sin(); // −1..1
                                                           // Map: t → x in [0, w];  y_n → y in [0, h] with y_n=−1 at
                                                           // the top and y_n=+1 at the bottom of egui's screen.
                                                           // Y-up: top = high Y, so flip: y = (1 − y_n) · 0.5 · h.
                    let x = t * w;
                    let y = (1.0 - y_n) * 0.5 * h;

                    if let Some((px, py)) = prev {
                        // Colour based on midpoint's x-offset from centre.
                        let mid_x = (px + x) * 0.5;
                        let dist_n = ((mid_x / w) * 2.0 - 1.0).abs() as f32; // 0..1
                        let col = Color::rgb(
                            lerp_f32(center_color.r, outer_color.r, dist_n),
                            lerp_f32(center_color.g, outer_color.g, dist_n),
                            lerp_f32(center_color.b, outer_color.b, dist_n),
                        );
                        ctx.set_stroke_color(col);
                        ctx.begin_path();
                        ctx.move_to(px, py);
                        ctx.line_to(x, y);
                        ctx.stroke();
                    }
                    prev = Some((x, y));
                }
            } else {
                ctx.set_stroke_color(base);
                ctx.set_line_width(thickness);
                ctx.begin_path();
                for i in 0..=n {
                    let t = i as f64 / n as f64;
                    let amp = (time * speed * mode).sin() / mode;
                    let y_n = amp * (t * PI * mode).sin();
                    let x = t * w;
                    let y = (1.0 - y_n) * 0.5 * h;
                    if i == 0 {
                        ctx.move_to(x, y);
                    } else {
                        ctx.line_to(x, y);
                    }
                }
                ctx.stroke();
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }
}

#[inline]
fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Build the Dancing Strings demo — Colored checkbox above the animated canvas.
pub fn dancing_strings(font: Arc<Font>) -> Box<dyn Widget> {
    use std::cell::Cell;
    use std::rc::Rc;

    let colored = Rc::new(Cell::new(false));

    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(8.0)
        .with_panel_bg();

    let cell = Rc::clone(&colored);
    col.push(
        Box::new(
            agg_gui::Checkbox::new("Colored", Arc::clone(&font), false)
                .with_state_cell(Rc::clone(&colored))
                .on_change(move |v| cell.set(v)),
        ),
        0.0,
    );

    col.push(Box::new(DancingStrings::new(Rc::clone(&colored))), 1.0);

    Box::new(col)
}
