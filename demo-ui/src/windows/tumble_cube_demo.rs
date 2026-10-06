//! Tumble Cube demo — agg-gui's view cube driving a toy 3-D view.
//!
//! Not an egui demo (egui has no view cube); it exists to show
//! [`TumbleCube`] working.  [`DemoCamera`] is the smallest useful
//! [`TumbleCubeCamera`]: it holds a world → view rotation, turns it like
//! MatterCAD's turntable drag (spin about world Z, tilt about the screen X
//! axis) and animates clicks with the same 0.25 s quaternion slerp as the
//! C# `AnimateRotation`.  [`AxesView`] draws the world axes and a bed
//! square through that rotation, with the cube in its top-right corner, so
//! the cube can be checked against the scene it controls.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::widgets::tumble_cube::math::{self, Mat3, Quat};
use agg_gui::{
    Color, DrawCtx, Event, EventResult, Point, Rect, Size, TumbleCube, TumbleCubeCamera, Widget,
};
use web_time::Instant;

/// Radians of turn per pixel of drag.
const DRAG_RADIANS_PER_PX: f64 = 0.01;
/// The C# `RunCameraMove(.25, ...)` duration.
const ANIMATION_SECONDS: f64 = 0.25;

/// A minimal host camera for the cube.
pub(crate) struct DemoCamera {
    rotation: Mat3,
    last_drag: Option<Point>,
    animation: Option<(Quat, Quat, Instant)>,
}

impl DemoCamera {
    pub(crate) fn new() -> Self {
        // A three-quarter view from the front-right, above the bed.
        let rotation = math::look_at([-0.6, 1.0, -0.7], [0.0, 0.0, 1.0]);
        Self {
            rotation,
            last_drag: None,
            animation: None,
        }
    }

    /// Advance any running turn and return the current rotation.
    fn current(&mut self) -> Mat3 {
        if let Some((from, to, start)) = self.animation {
            let t = (start.elapsed().as_secs_f64() / ANIMATION_SECONDS).min(1.0);
            self.rotation = math::mat3_from_quat(math::slerp(from, to, t));
            if t >= 1.0 {
                self.animation = None;
            } else {
                agg_gui::animation::request_draw();
            }
        }
        self.rotation
    }
}

impl TumbleCubeCamera for DemoCamera {
    fn view_rotation(&self) -> Mat3 {
        self.rotation
    }

    fn begin_rotate(&mut self, pos: Point) {
        // Taking hold of the view stops a turn in progress.
        self.animation = None;
        self.last_drag = Some(pos);
    }

    fn rotate(&mut self, pos: Point) {
        let Some(last) = self.last_drag.replace(pos) else {
            return;
        };
        let (a, b) = (
            (pos.x - last.x) * DRAG_RADIANS_PER_PX,
            -(pos.y - last.y) * DRAG_RADIANS_PER_PX,
        );
        let (ca, sa, cb, sb) = (a.cos(), a.sin(), b.cos(), b.sin());
        // Row-vector rotations: spin about world Z first, then tilt about
        // the view's X axis (the C# turntable branch).
        let spin = [[ca, sa, 0.0], [-sa, ca, 0.0], [0.0, 0.0, 1.0]];
        let tilt = [[1.0, 0.0, 0.0], [0.0, cb, sb], [0.0, -sb, cb]];
        self.rotation = math::mul(&math::mul(&spin, &self.rotation), &tilt);
    }

    fn end_rotate(&mut self) {
        self.last_drag = None;
    }

    fn animate_rotation(&mut self, target: Mat3) {
        let from = math::quat_from_mat3(&self.rotation);
        self.animation = Some((from, math::quat_from_mat3(&target), Instant::now()));
        agg_gui::animation::request_draw();
    }
}

/// Orthographic axes + bed view with the cube overlaid top-right.
struct AxesView {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    camera: Rc<RefCell<DemoCamera>>,
}

impl Widget for AxesView {
    fn type_name(&self) -> &'static str {
        "TumbleCubeDemoView"
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
        self.bounds = Rect::new(
            self.bounds.x,
            self.bounds.y,
            available.width,
            available.height,
        );
        if let Some(cube) = self.children.first_mut() {
            let s = cube.layout(available);
            cube.set_bounds(Rect::new(
                available.width - s.width - 8.0,
                available.height - s.height - 8.0,
                s.width,
                s.height,
            ));
        }
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let rot = self.camera.borrow_mut().current();
        let (w, h) = (self.bounds.width, self.bounds.height);
        ctx.set_fill_color(ctx.visuals().panel_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();
        let scale = w.min(h) * 0.3;
        let (cx, cy) = (w * 0.4, h * 0.45);
        let project = |p: [f64; 3]| {
            let v = math::transform(p, &rot);
            (cx + v[0] * scale, cy + v[1] * scale)
        };
        ctx.set_line_width(1.5);
        ctx.set_stroke_color(ctx.visuals().text_dim);
        ctx.begin_path();
        let bed = [
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ];
        for (i, p) in bed.iter().enumerate() {
            let (x, y) = project(*p);
            if i == 0 {
                ctx.move_to(x, y);
            } else {
                ctx.line_to(x, y);
            }
        }
        ctx.close_path();
        ctx.stroke();
        let axes = [
            ([1.2, 0.0, 0.0], Color::rgb(0.85, 0.2, 0.2)),
            ([0.0, 1.2, 0.0], Color::rgb(0.2, 0.7, 0.25)),
            ([0.0, 0.0, 1.2], Color::rgb(0.25, 0.4, 0.95)),
        ];
        ctx.set_line_width(3.0);
        for (tip, color) in axes {
            let (x0, y0) = project([0.0; 3]);
            let (x1, y1) = project(tip);
            ctx.set_stroke_color(color);
            ctx.begin_path();
            ctx.move_to(x0, y0);
            ctx.line_to(x1, y1);
            ctx.stroke();
        }
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Build the Tumble Cube demo window content.
pub fn tumble_cube_demo() -> Box<dyn Widget> {
    let camera = Rc::new(RefCell::new(DemoCamera::new()));
    let cube = TumbleCube::new(camera.clone());
    Box::new(AxesView {
        bounds: Rect::default(),
        children: vec![Box::new(cube)],
        camera,
    })
}
