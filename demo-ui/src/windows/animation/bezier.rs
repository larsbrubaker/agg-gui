//! The "Bézier Curve" editor demo — an interactive quadratic/cubic Bézier
//! curve whose control points can be dragged, matching egui's `PaintBezier`
//! (`paint_bezier.rs`).
//!
//! Part of the [`super`] animation demo group.  Beyond the draggable control
//! points this reproduces egui's degree radio (Quadratic 3-point / Cubic
//! 4-point), the translucent fill of the closed curve, the visual
//! bounding-box rectangle, and the auxiliary polyline through all control
//! points.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Color, DrawCtx, Event, EventResult, FlexColumn, Font, Label, MouseButton, Point, RadioGroup,
    Rect, Size, Widget,
};

// ---------------------------------------------------------------------------
// BezierCanvas
// ---------------------------------------------------------------------------

/// An interactive Bézier curve editor (quadratic or cubic).
///
/// The control points (P0..P3) can be dragged with the mouse.  In quadratic
/// mode the first three points are used; in cubic mode all four.  A single
/// auxiliary polyline connects the control points (egui's `aux_stroke`), the
/// closed curve is filled with a translucent color, and a bounding-box
/// rectangle is stroked around the curve's visual bounds.
///
/// Coordinates in `pts` are in local canvas space (Y-up, origin bottom-left).
struct BezierCanvas {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    /// Control-point positions in local canvas coordinates (Y-up).
    pts: [(f64, f64); 4],
    /// Index of the control point currently being dragged, if any.
    dragging: Option<usize>,
    /// Shared selection index from the degree radio: 0 = quadratic (3 points),
    /// 1 = cubic (4 points).  Read every paint so the radio drives the canvas.
    degree_idx: Rc<Cell<usize>>,
}

impl BezierCanvas {
    /// Snap-radius for starting a drag (pixels).
    const SNAP_R: f64 = 12.0;
    /// Drawn radius of the inner handle points (P1, P2).
    const HANDLE_R: f64 = 8.0;
    /// Drawn radius of the endpoint handle points (P0, P3).
    const ENDPOINT_R: f64 = 10.0;

    // egui `PaintBezier` default colors (see paint_bezier.rs `Default`).
    // `linear_multiply(0.25)` on an opaque color yields ~0.25 alpha, so the
    // fill / aux / bbox colors are translucent.
    fn curve_stroke() -> Color {
        Color::rgb(25.0 / 255.0, 200.0 / 255.0, 100.0 / 255.0)
    }
    fn fill_color() -> Color {
        Color::rgba(50.0 / 255.0, 100.0 / 255.0, 150.0 / 255.0, 0.25)
    }
    fn aux_color() -> Color {
        Color::rgba(1.0, 0.0, 0.0, 0.25)
    }
    fn bbox_color() -> Color {
        // egui LIGHT_GREEN is (144, 238, 144); multiplied to ~0.25 alpha.
        Color::rgba(144.0 / 255.0, 238.0 / 255.0, 144.0 / 255.0, 0.25)
    }

    fn new(degree_idx: Rc<Cell<usize>>) -> Self {
        // Initial control points chosen so the curve opens upward in the
        // center of a 360×290 canvas (Y-up: y=0 is the bottom).
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            pts: [
                (80.0, 90.0),   // P0 — bottom-left anchor
                (140.0, 210.0), // P1 — upper-left handle (pulls curve up)
                (220.0, 210.0), // P2 — upper-right handle
                (280.0, 90.0),  // P3 — bottom-right anchor
            ],
            dragging: None,
            degree_idx,
        }
    }

    /// Number of active control points: 3 for quadratic, 4 for cubic.
    fn degree(&self) -> usize {
        if self.degree_idx.get() == 0 {
            3
        } else {
            4
        }
    }

    /// Return the index of the nearest *active* control point within `SNAP_R`
    /// of `pos`, or `None` if no point is close enough.
    fn nearest(&self, pos: Point) -> Option<usize> {
        let n = self.degree();
        self.pts
            .iter()
            .take(n)
            .enumerate()
            .filter_map(|(i, &(px, py))| {
                let d = ((pos.x - px).powi(2) + (pos.y - py).powi(2)).sqrt();
                if d <= Self::SNAP_R {
                    Some((i, d))
                } else {
                    None
                }
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(i, _)| i)
    }

    /// Evaluate the curve at parameter `t` in [0, 1] for the current degree.
    fn eval(&self, t: f64) -> (f64, f64) {
        let [(x0, y0), (x1, y1), (x2, y2), (x3, y3)] = self.pts;
        let mt = 1.0 - t;
        if self.degree() == 3 {
            // Quadratic: (1-t)^2 P0 + 2(1-t)t P1 + t^2 P2.
            let a = mt * mt;
            let b = 2.0 * mt * t;
            let c = t * t;
            (a * x0 + b * x1 + c * x2, a * y0 + b * y1 + c * y2)
        } else {
            // Cubic: (1-t)^3 P0 + 3(1-t)^2 t P1 + 3(1-t)t^2 P2 + t^3 P3.
            let a = mt * mt * mt;
            let b = 3.0 * mt * mt * t;
            let c = 3.0 * mt * t * t;
            let d = t * t * t;
            (
                a * x0 + b * x1 + c * x2 + d * x3,
                a * y0 + b * y1 + c * y2 + d * y3,
            )
        }
    }

    /// Tight visual bounds of the flattened curve, expanded by half the stroke
    /// width — the analogue of egui's `visual_bounding_rect`.
    fn curve_bounds(&self, stroke_w: f64) -> (f64, f64, f64, f64) {
        const SAMPLES: usize = 64;
        let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
        let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for s in 0..=SAMPLES {
            let (x, y) = self.eval(s as f64 / SAMPLES as f64);
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        let pad = stroke_w * 0.5;
        (min_x - pad, min_y - pad, max_x + pad, max_y + pad)
    }
}

impl Widget for BezierCanvas {
    fn type_name(&self) -> &'static str {
        "BezierCanvas"
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

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let degree = self.degree();
        let [(x0, y0), (x1, y1), (x2, y2), (x3, y3)] = self.pts;
        let curve_w = 1.5;

        // Background.
        ctx.set_fill_color(v.bg_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // Closed curve: build the path once, then fill (translucent) and
        // stroke it.  `close_path` adds the straight segment back to P0 so
        // both the fill and the stroke are closed, matching egui's
        // `from_points_stroke(.., closed = true, ..)`.
        let build_curve = |ctx: &mut dyn DrawCtx| {
            ctx.begin_path();
            ctx.move_to(x0, y0);
            if degree == 3 {
                ctx.quad_to(x1, y1, x2, y2);
            } else {
                ctx.cubic_to(x1, y1, x2, y2, x3, y3);
            }
            ctx.close_path();
        };

        ctx.set_fill_color(Self::fill_color());
        build_curve(ctx);
        ctx.fill();

        ctx.set_stroke_color(Self::curve_stroke());
        ctx.set_line_width(curve_w);
        build_curve(ctx);
        ctx.stroke();

        // Bounding-box rectangle around the curve's visual bounds.
        //
        // Deviation: egui's default `bounding_box_stroke` width is 0.0 (so the
        // box is invisible until the user widens it in the Colors panel).  We
        // omit that optional Colors panel, so we draw the box at width 1.0 to
        // make this feature visible by default.
        let (bx0, by0, bx1, by1) = self.curve_bounds(curve_w);
        ctx.set_stroke_color(Self::bbox_color());
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rect(bx0, by0, bx1 - bx0, by1 - by0);
        ctx.stroke();

        // Auxiliary polyline through all active control points (egui draws one
        // solid `PathShape::line` — replaces the two dashed segments we had).
        ctx.set_stroke_color(Self::aux_color());
        ctx.set_line_width(1.0);
        ctx.begin_path();
        for (i, &(px, py)) in self.pts.iter().take(degree).enumerate() {
            if i == 0 {
                ctx.move_to(px, py);
            } else {
                ctx.line_to(px, py);
            }
        }
        ctx.stroke();

        // Control point circles.
        let hovered_pt = self.dragging; // highlight dragged point
        for (i, &(px, py)) in self.pts.iter().take(degree).enumerate() {
            let is_endpoint = i == 0 || i == degree - 1;
            let r = if is_endpoint {
                Self::ENDPOINT_R
            } else {
                Self::HANDLE_R
            };
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
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(idx) = self.nearest(*pos) {
                    self.dragging = Some(idx);
                    agg_gui::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseMove { pos } => {
                if let Some(idx) = self.dragging {
                    let clamped_x = pos.x.clamp(0.0, self.bounds.width);
                    let clamped_y = pos.y.clamp(0.0, self.bounds.height);
                    let next = (clamped_x, clamped_y);
                    if self.pts[idx] != next {
                        self.pts[idx] = next;
                        agg_gui::animation::request_draw();
                    }
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                if self.dragging.is_some() {
                    self.dragging = None;
                    agg_gui::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }
}

#[cfg(test)]
mod bezier_tests {
    use super::*;

    fn cubic_canvas() -> BezierCanvas {
        // degree index 1 → cubic (4 points).
        BezierCanvas::new(Rc::new(Cell::new(1)))
    }

    #[test]
    fn dragging_control_point_requests_draw() {
        let mut canvas = cubic_canvas();
        canvas.layout(Size::new(360.0, 290.0));

        agg_gui::animation::clear_draw_request();
        assert_eq!(
            canvas.on_event(&Event::MouseDown {
                pos: Point::new(80.0, 90.0),
                button: MouseButton::Left,
                modifiers: Default::default(),
            }),
            EventResult::Consumed
        );
        assert!(agg_gui::animation::wants_draw());

        agg_gui::animation::clear_draw_request();
        assert_eq!(
            canvas.on_event(&Event::MouseMove {
                pos: Point::new(100.0, 110.0),
            }),
            EventResult::Consumed
        );
        assert_eq!(canvas.pts[0], (100.0, 110.0));
        assert!(agg_gui::animation::wants_draw());

        agg_gui::animation::clear_draw_request();
        assert_eq!(
            canvas.on_event(&Event::MouseUp {
                pos: Point::new(100.0, 110.0),
                button: MouseButton::Left,
                modifiers: Default::default(),
            }),
            EventResult::Consumed
        );
        assert!(agg_gui::animation::wants_draw());
    }

    #[test]
    fn quadratic_mode_ignores_fourth_point() {
        // degree index 0 → quadratic (3 points).  P3 must not be draggable.
        let mut canvas = BezierCanvas::new(Rc::new(Cell::new(0)));
        canvas.layout(Size::new(360.0, 290.0));
        assert_eq!(canvas.degree(), 3);
        // P3 is at (280, 90); grabbing there should miss in quadratic mode.
        assert_eq!(canvas.nearest(Point::new(280.0, 90.0)), None);
        // P0 is still grabbable.
        assert_eq!(canvas.nearest(Point::new(80.0, 90.0)), Some(0));
    }

    #[test]
    fn curve_bounds_contains_endpoints() {
        let canvas = cubic_canvas();
        let (bx0, by0, bx1, by1) = canvas.curve_bounds(1.0);
        // Endpoints P0 (80,90) and P3 (280,90) lie on the curve, so the
        // bounds must enclose them.
        assert!(bx0 <= 80.0 && bx1 >= 280.0);
        assert!(by0 <= 90.0 && by1 >= 90.0);
    }
}

/// Build the Bézier Curve demo — the degree radio and instructions above the
/// interactive canvas.
pub fn bezier_curve(font: Arc<Font>) -> Box<dyn Widget> {
    // Default degree is cubic (egui default `degree = 4`), i.e. radio index 1.
    let degree_idx = Rc::new(Cell::new(1_usize));

    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(8.0)
        .with_panel_bg();

    col.push(
        Box::new(
            RadioGroup::new(
                vec!["Quadratic Bézier", "Cubic Bézier"],
                degree_idx.get(),
                Arc::clone(&font),
            )
            .with_font_size(13.0)
            .with_selected_cell(Rc::clone(&degree_idx))
            .on_change(|_| agg_gui::animation::request_draw()),
        ),
        0.0,
    );

    col.push(
        Box::new(
            Label::new("Move the points by dragging them.", Arc::clone(&font)).with_font_size(12.0),
        ),
        0.0,
    );

    col.push(
        Box::new(
            Label::new(
                "Only convex curves can be accurately filled.",
                Arc::clone(&font),
            )
            .with_font_size(10.0)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(Box::new(BezierCanvas::new(Rc::clone(&degree_idx))), 1.0);

    Box::new(col)
}
