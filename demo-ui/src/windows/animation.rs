//! Animation demo windows: interactive Bézier curve editor, animated dancing
//! sine waves, and a freehand painting canvas.
//!
//! All three demos use custom `Widget` implementations with direct `DrawCtx`
//! calls — no layout children, just raw path drawing — to show what is possible
//! beyond the standard widget palette.
//!
//! Coordinate system: Y-up throughout (origin bottom-left, positive Y upward),
//! matching the agg-gui invariant.

use std::sync::Arc;

use agg_gui::{
    Button, Color, DrawCtx, Event, EventResult, FlexColumn, Font,
    Label, MouseButton, Point, Rect, Size,
    SizedBox, Widget,
};

// ---------------------------------------------------------------------------
// BezierCanvas
// ---------------------------------------------------------------------------

/// An interactive cubic Bézier curve editor.
///
/// The four control points (P0–P3) can be dragged with the mouse.  Guide lines
/// connect P0→P1 and P2→P3 to show the tangent handles.  P0 and P3 are the
/// endpoints (drawn slightly larger); P1 and P2 are the off-curve handles.
///
/// Coordinates in `pts` are in local canvas space (Y-up, origin bottom-left).
struct BezierCanvas {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    /// Control-point positions in local canvas coordinates (Y-up).
    pts:      [(f64, f64); 4],
    /// Index of the control point currently being dragged, if any.
    dragging: Option<usize>,
}

impl BezierCanvas {
    /// Snap-radius for starting a drag (pixels).
    const SNAP_R: f64 = 12.0;
    /// Drawn radius of the inner handle points (P1, P2).
    const HANDLE_R: f64 = 8.0;
    /// Drawn radius of the endpoint handle points (P0, P3).
    const ENDPOINT_R: f64 = 10.0;

    fn new() -> Self {
        // Initial control points chosen so the curve opens upward in the
        // center of a 360×290 canvas (Y-up: y=0 is the bottom).
        Self {
            bounds:   Rect::default(),
            children: Vec::new(),
            pts: [
                (80.0,  90.0),  // P0 — bottom-left anchor
                (140.0, 210.0), // P1 — upper-left handle (pulls curve up)
                (220.0, 210.0), // P2 — upper-right handle
                (280.0, 90.0),  // P3 — bottom-right anchor
            ],
            dragging: None,
        }
    }

    /// Return the index of the nearest control point within `SNAP_R` of `pos`,
    /// or `None` if no point is close enough.
    fn nearest(&self, pos: Point) -> Option<usize> {
        self.pts.iter()
            .enumerate()
            .filter_map(|(i, &(px, py))| {
                let d = ((pos.x - px).powi(2) + (pos.y - py).powi(2)).sqrt();
                if d <= Self::SNAP_R { Some((i, d)) } else { None }
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(i, _)| i)
    }
}

impl Widget for BezierCanvas {
    fn type_name(&self) -> &'static str { "BezierCanvas" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let [(x0,y0),(x1,y1),(x2,y2),(x3,y3)] = self.pts;

        // Background.
        ctx.set_fill_color(v.bg_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // Guide lines P0→P1 and P2→P3 (dashed appearance via short segments).
        let guide_color = Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.4);
        ctx.set_stroke_color(guide_color);
        ctx.set_line_width(1.0);

        // Draw dashed lines by alternating drawn/skipped segments (~8px each).
        for (ax, ay, bx, by) in [(x0,y0,x1,y1), (x2,y2,x3,y3)] {
            let dx = bx - ax;
            let dy = by - ay;
            let len = (dx * dx + dy * dy).sqrt().max(1.0);
            let seg = 8.0_f64;
            let steps = (len / seg).ceil() as usize;
            let mut draw = true;
            let mut prev_x = ax;
            let mut prev_y = ay;
            for s in 1..=steps {
                let t = (s as f64 * seg).min(len) / len;
                let nx = ax + dx * t;
                let ny = ay + dy * t;
                if draw {
                    ctx.begin_path();
                    ctx.move_to(prev_x, prev_y);
                    ctx.line_to(nx, ny);
                    ctx.stroke();
                }
                draw = !draw;
                prev_x = nx;
                prev_y = ny;
            }
        }

        // Cubic Bézier curve.
        ctx.set_stroke_color(v.accent);
        ctx.set_line_width(2.5);
        ctx.begin_path();
        ctx.move_to(x0, y0);
        ctx.cubic_to(x1, y1, x2, y2, x3, y3);
        ctx.stroke();

        // Control point circles.
        let hovered_pt = self.dragging; // highlight dragged point
        for (i, &(px, py)) in self.pts.iter().enumerate() {
            let is_endpoint = i == 0 || i == 3;
            let r = if is_endpoint { Self::ENDPOINT_R } else { Self::HANDLE_R };
            let fill = if hovered_pt == Some(i) {
                v.widget_bg_hovered
            } else {
                v.accent
            };
            ctx.set_fill_color(fill);
            ctx.set_stroke_color(v.window_fill);
            ctx.set_line_width(1.5);
            ctx.begin_path();
            ctx.circle(px, py, r);
            ctx.fill_and_stroke();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                if let Some(idx) = self.nearest(*pos) {
                    self.dragging = Some(idx);
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseMove { pos } => {
                if let Some(idx) = self.dragging {
                    let clamped_x = pos.x.clamp(0.0, self.bounds.width);
                    let clamped_y = pos.y.clamp(0.0, self.bounds.height);
                    self.pts[idx] = (clamped_x, clamped_y);
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp { button: MouseButton::Left, .. } => {
                if self.dragging.is_some() {
                    self.dragging = None;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0 && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0 && local_pos.y <= self.bounds.height
    }
}

/// Build the Bézier Curve demo — a label above the interactive canvas.
pub fn bezier_curve(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(8.0)
        .with_panel_bg();

    col.push(Box::new(Label::new(
        "Drag the control points",
        Arc::clone(&font),
    ).with_font_size(12.0)), 0.0);

    col.push(Box::new(BezierCanvas::new()), 1.0);

    Box::new(col)
}

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
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    start:    web_time::Instant,
    colored:  std::rc::Rc<std::cell::Cell<bool>>,
}

impl DancingStrings {
    fn new(colored: std::rc::Rc<std::cell::Cell<bool>>) -> Self {
        Self {
            bounds:   Rect::default(),
            children: Vec::new(),
            start:    web_time::Instant::now(),
            colored,
        }
    }
}

impl Widget for DancingStrings {
    fn type_name(&self) -> &'static str { "DancingStrings" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

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
        let center_color = Color::rgb(0x5B as f32 / 255.0, 0xCE as f32 / 255.0, 0xFA as f32 / 255.0);
        let outer_color  = Color::rgb(0xF5 as f32 / 255.0, 0xA9 as f32 / 255.0, 0xB8 as f32 / 255.0);

        let speed = 1.5_f64;
        let n     = 120_usize;

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
                    let t     = i as f64 / n as f64;
                    let amp   = (time * speed * mode).sin() / mode;
                    let y_n   = amp * (t * PI * mode).sin();      // −1..1
                    // Map: t → x in [0, w];  y_n → y in [0, h] with y_n=−1 at
                    // the top and y_n=+1 at the bottom of egui's screen.
                    // Y-up: top = high Y, so flip: y = (1 − y_n) · 0.5 · h.
                    let x = t * w;
                    let y = (1.0 - y_n) * 0.5 * h;

                    if let Some((px, py)) = prev {
                        // Colour based on midpoint's x-offset from centre.
                        let mid_x    = (px + x) * 0.5;
                        let dist_n   = ((mid_x / w) * 2.0 - 1.0).abs() as f32; // 0..1
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
                    let t     = i as f64 / n as f64;
                    let amp   = (time * speed * mode).sin() / mode;
                    let y_n   = amp * (t * PI * mode).sin();
                    let x = t * w;
                    let y = (1.0 - y_n) * 0.5 * h;
                    if i == 0 { ctx.move_to(x, y); } else { ctx.line_to(x, y); }
                }
                ctx.stroke();
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0 && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0 && local_pos.y <= self.bounds.height
    }
}

#[inline]
fn lerp_f32(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }

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
    col.push(Box::new(
        agg_gui::Checkbox::new("Colored", Arc::clone(&font), false)
            .with_state_cell(Rc::clone(&colored))
            .on_change(move |v| cell.set(v))
    ), 0.0);

    col.push(Box::new(DancingStrings::new(Rc::clone(&colored))), 1.0);

    Box::new(col)
}

// ---------------------------------------------------------------------------
// PaintCanvas
// ---------------------------------------------------------------------------

/// A freehand drawing canvas.
///
/// Each mouse-drag gesture creates a new stroke stored as a list of points.
/// On paint, every stroke is replayed as a connected path.  A "Clear" button
/// (built outside this widget) can reset the stroke list via an `Rc<RefCell>`.
struct PaintCanvas {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    /// Completed and in-progress strokes, each a sequence of (x, y) points.
    strokes:  Vec<Vec<(f64, f64)>>,
    /// Whether the left mouse button is currently held inside the canvas.
    painting: bool,
}

impl PaintCanvas {
    fn new() -> Self {
        Self {
            bounds:   Rect::default(),
            children: Vec::new(),
            strokes:  Vec::new(),
            painting: false,
        }
    }

    fn clear(&mut self) {
        self.strokes.clear();
        self.painting = false;
    }
}

impl Widget for PaintCanvas {
    fn type_name(&self) -> &'static str { "PaintCanvas" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Background.
        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // Thin border.
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.stroke();

        // Draw all strokes.
        ctx.set_stroke_color(v.accent);
        ctx.set_line_width(2.5);
        for stroke in &self.strokes {
            if stroke.len() < 2 { continue; }
            ctx.begin_path();
            let (fx, fy) = stroke[0];
            ctx.move_to(fx, fy);
            for &(px, py) in &stroke[1..] {
                ctx.line_to(px, py);
            }
            ctx.stroke();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                self.painting = true;
                self.strokes.push(vec![(pos.x, pos.y)]);
                EventResult::Consumed
            }
            Event::MouseMove { pos } => {
                if self.painting {
                    if let Some(stroke) = self.strokes.last_mut() {
                        stroke.push((pos.x, pos.y));
                    }
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp { button: MouseButton::Left, .. } => {
                if self.painting {
                    self.painting = false;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0 && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0 && local_pos.y <= self.bounds.height
    }
}

// ---------------------------------------------------------------------------
// PaintingRoot — wraps the canvas with a Clear button
// ---------------------------------------------------------------------------

/// Top-level widget for the Painting demo.
///
/// Holds the `PaintCanvas` as a direct child (index 1 after the Clear button
/// container), and handles the Clear button click by accessing the canvas via
/// `children_mut`.
struct PaintingRoot {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>, // [0] = FlexColumn toolbar, [1] = PaintCanvas
}

impl PaintingRoot {
    fn new(toolbar: Box<dyn Widget>, canvas: Box<dyn Widget>) -> Self {
        Self {
            bounds:   Rect::default(),
            children: vec![toolbar, canvas],
        }
    }
}

impl Widget for PaintingRoot {
    fn type_name(&self) -> &'static str { "PaintingRoot" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        // Toolbar gets its natural height; canvas fills the rest.
        let toolbar_size = self.children[0].layout(Size::new(available.width, 40.0));
        let canvas_h = (available.height - toolbar_size.height).max(0.0);
        let canvas_size = self.children[1].layout(Size::new(available.width, canvas_h));
        // Position toolbar at top, canvas below (Y-up: canvas at y=0, toolbar above).
        self.children[1].set_bounds(Rect::new(0.0, 0.0, canvas_size.width, canvas_size.height));
        self.children[0].set_bounds(Rect::new(
            0.0,
            canvas_size.height,
            toolbar_size.width,
            toolbar_size.height,
        ));
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_fill_color(v.panel_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Build the Painting demo — a toolbar with a Clear button above the canvas.
///
/// Because `PaintCanvas` owns its stroke list, the Clear button uses a shared
/// `Rc<Cell<bool>>` flag: the button sets it, and on the next paint pass the
/// canvas checks and clears itself.
pub fn painting(font: Arc<Font>) -> Box<dyn Widget> {
    use std::cell::Cell;
    use std::rc::Rc;

    let clear_flag = Rc::new(Cell::new(false));

    // Canvas that polls the clear flag on every layout/paint.
    struct ClearablePaintCanvas {
        inner: PaintCanvas,
        flag:  Rc<Cell<bool>>,
    }

    impl Widget for ClearablePaintCanvas {
        fn type_name(&self) -> &'static str { "ClearablePaintCanvas" }
        fn bounds(&self) -> Rect { self.inner.bounds }
        fn set_bounds(&mut self, b: Rect) { self.inner.bounds = b; }
        fn children(&self) -> &[Box<dyn Widget>] { &self.inner.children }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.inner.children }

        fn layout(&mut self, available: Size) -> Size {
            if self.flag.get() {
                self.inner.clear();
                self.flag.set(false);
            }
            self.inner.layout(available)
        }

        fn paint(&mut self, ctx: &mut dyn DrawCtx) {
            self.inner.paint(ctx)
        }

        fn on_event(&mut self, event: &Event) -> EventResult {
            self.inner.on_event(event)
        }

        fn hit_test(&self, local_pos: Point) -> bool {
            self.inner.hit_test(local_pos)
        }
    }

    let flag_for_btn = Rc::clone(&clear_flag);
    let toolbar = {
        let row = agg_gui::FlexRow::new().with_gap(8.0).with_padding(6.0)
            .add(Box::new(SizedBox::new().with_height(26.0).with_child(Box::new(
                Button::new("Clear", Arc::clone(&font))
                    .with_font_size(12.0)
                    .on_click(move || { flag_for_btn.set(true); })
            ))));
        Box::new(row) as Box<dyn Widget>
    };

    let canvas = Box::new(ClearablePaintCanvas {
        inner: PaintCanvas::new(),
        flag:  Rc::clone(&clear_flag),
    });

    Box::new(PaintingRoot::new(toolbar, canvas))
}
