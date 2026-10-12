//! Port of agg-sharp `WindowWidgetResizeTests`: a drag of one of a `Window`'s
//! eight grab handles tracks the mouse exactly — each screen pixel of mouse
//! travel is one pixel of size (and, for the left and bottom handles, one
//! pixel of movement), and only while the button that started the drag is
//! held.
//!
//! The drags run through the real `App` pointer path. C#'s `SystemWindow`
//! is Y-up like agg-gui's logical space; the `App` entry points take Y-down
//! screen coordinates, so every press and move goes through `screen_y`. agg-gui's
//! grab handles are not child widgets but `RESIZE_EDGE` (6 px) zones along
//! the window's edges, so "the middle of the grab control" is 3 px in from
//! each edge it touches. C#'s `MouseButtons.None` move to (-10, -10) — what a
//! platform sink sends when the pointer leaves — is `App::on_mouse_leave`.
//!
//! `HoveringAfterALostMouseUpDoesNotResizeTheWindow` is skipped as a duplicate
//! of `agg-gui-automation/tests/capture_lost_drag_tests.rs::rust_only_a_resize_drag_ends_on_capture_loss`.
//!
//! `rust_only_a_half_unit_edge_drag_leaves_the_other_edge_alone` pins a fix:
//! resized bounds used to round their left and width separately, so a drag
//! landing on a half unit (a 5 px step in a container zoomed 2x) moved the
//! edge that was not being dragged by a whole unit.

use crate::event::{Event, EventResult, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::window::Window;
use crate::{App, DrawCtx, SizedBox, Stack, TransAffine};
use std::sync::Arc;

const STEP_COUNT: i32 = 10;
const STEP_SIZE: f64 = 5.0;
const VP_W: f64 = 800.0;
const VP_H: f64 = 600.0;
/// Half the 6 px resize zone: the middle of a grab handle across its edge.
const GRAB_MID: f64 = 3.0;
/// `Window`'s minimum width (agg-sharp `MinimumSize.X`).
const MIN_W: f64 = 120.0;

#[derive(Clone, Copy, PartialEq)]
enum H {
    Left,
    Right,
    Stretch,
}

#[derive(Clone, Copy, PartialEq)]
enum V {
    Bottom,
    Top,
    Stretch,
}

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(super::TEST_FONT).expect("test font"))
}

fn new_window() -> Window {
    // Windows register as snap targets in a thread-local list; drop the ones
    // an earlier test on this thread left, which would pull these edges.
    crate::snap::clear_targets_for_testing();
    // C#: new RectangleDouble(100, 100, 400, 400)
    Window::new("Resize", font(), Box::new(SizedBox::new()))
        .with_bounds(Rect::new(100.0, 100.0, 300.0, 300.0))
}

/// The middle of the grab handle with the given anchors, window-local Y-up.
fn grab_local(bounds: Rect, h: H, v: V) -> Point {
    let x = match h {
        H::Left => GRAB_MID,
        H::Right => bounds.width - GRAB_MID,
        H::Stretch => bounds.width / 2.0,
    };
    let y = match v {
        V::Bottom => GRAB_MID,
        V::Top => bounds.height - GRAB_MID,
        V::Stretch => bounds.height / 2.0,
    };
    Point::new(x, y)
}

fn window_bounds(app: &App) -> Rect {
    app.root().children()[0].bounds()
}

fn screen_y(vp_h: f64, y_up: f64) -> f64 {
    vp_h - y_up
}

/// Builds a window on an app and presses the middle of the grab handle with
/// the given anchors, returning the Y-up position the mouse was pressed at.
fn start_drag(h: H, v: V) -> (App, Point) {
    let mut app = App::new(Box::new(Stack::new().add(Box::new(new_window()))));
    app.layout(Size::new(VP_W, VP_H));
    let b = window_bounds(&app);
    let local = grab_local(b, h, v);
    let mouse = Point::new(b.x + local.x, b.y + local.y);
    app.on_mouse_move(mouse.x, screen_y(VP_H, mouse.y));
    app.on_mouse_down(
        mouse.x,
        screen_y(VP_H, mouse.y),
        MouseButton::Left,
        Modifiers::default(),
    );
    (app, mouse)
}

fn move_to(app: &mut App, mouse: Point) {
    app.on_mouse_move(mouse.x, screen_y(VP_H, mouse.y));
}

/// Presses the grab handle with the given anchors and walks the mouse
/// `STEP_COUNT` steps of `mouse_step`, asserting after every step that the
/// window's size and position have moved exactly one step further — never
/// backwards, never twice as far.
fn assert_drag_tracks_mouse(
    h: H,
    v: V,
    mouse_step: (f64, f64),
    expected_size_step: (f64, f64),
    expected_position_step: (f64, f64),
) {
    let (mut app, mut mouse) = start_drag(h, v);
    let start = window_bounds(&app);
    for step in 1..=STEP_COUNT {
        let s = step as f64;
        mouse = Point::new(mouse.x + mouse_step.0, mouse.y + mouse_step.1);
        move_to(&mut app, mouse);
        let b = window_bounds(&app);
        let expected = [
            ("width", start.width + expected_size_step.0 * s, b.width),
            ("height", start.height + expected_size_step.1 * s, b.height),
            ("left", start.x + expected_position_step.0 * s, b.x),
            ("bottom", start.y + expected_position_step.1 * s, b.y),
        ];
        for (what, want, got) in expected {
            assert!(
                (got - want).abs() <= 1.0,
                "after {step} steps of {mouse_step:?} the {what} should be {want} but was {got}"
            );
        }
    }
    app.on_mouse_up(
        mouse.x,
        screen_y(VP_H, mouse.y),
        MouseButton::Left,
        Modifiers::default(),
    );
}

#[test]
fn bottom_right_grab_tracks_mouse_in_screen_space() {
    // Down and to the right: the right edge follows the mouse out, the bottom
    // edge follows it down, so both dimensions grow and the window's position
    // drops with the bottom edge.
    let s = STEP_SIZE;
    assert_drag_tracks_mouse(H::Right, V::Bottom, (s, -s), (s, s), (0.0, -s));
}

#[test]
fn top_left_grab_tracks_mouse_in_screen_space() {
    // Up and to the left: the left edge follows the mouse out (so the position
    // moves with it) and the top edge follows it up.
    let s = STEP_SIZE;
    assert_drag_tracks_mouse(H::Left, V::Top, (-s, s), (s, s), (-s, 0.0));
}

#[test]
fn bottom_left_grab_tracks_mouse_in_screen_space() {
    let s = STEP_SIZE;
    assert_drag_tracks_mouse(H::Left, V::Bottom, (-s, -s), (s, s), (-s, -s));
}

#[test]
fn top_right_grab_tracks_mouse_in_screen_space() {
    let s = STEP_SIZE;
    assert_drag_tracks_mouse(H::Right, V::Top, (s, s), (s, s), (0.0, 0.0));
}

#[test]
fn left_grab_tracks_mouse_in_screen_space() {
    let s = STEP_SIZE;
    assert_drag_tracks_mouse(H::Left, V::Stretch, (-s, 0.0), (s, 0.0), (-s, 0.0));
}

#[test]
fn bottom_grab_tracks_mouse_in_screen_space() {
    let s = STEP_SIZE;
    assert_drag_tracks_mouse(H::Stretch, V::Bottom, (0.0, -s), (0.0, s), (0.0, -s));
}

#[test]
fn right_grab_tracks_mouse_in_screen_space() {
    let s = STEP_SIZE;
    assert_drag_tracks_mouse(H::Right, V::Stretch, (s, 0.0), (s, 0.0), (0.0, 0.0));
}

#[test]
fn top_grab_tracks_mouse_in_screen_space() {
    let s = STEP_SIZE;
    assert_drag_tracks_mouse(H::Stretch, V::Top, (0.0, s), (0.0, s), (0.0, 0.0));
}

// Regression: the window jumped to a sliver and then chased the pointer
// around. A pointer leaving the platform window reaches the tree as a move to
// a point far outside it, and the grab handle took that for a drag of the
// whole window's width.
#[test]
fn pointer_leaving_the_window_does_not_resize_it() {
    let (mut app, mut mouse) = start_drag(H::Right, V::Bottom);
    mouse = Point::new(mouse.x + STEP_SIZE, mouse.y - STEP_SIZE);
    move_to(&mut app, mouse);
    let dragged = window_bounds(&app);

    // what a platform shell sends when the pointer leaves the window
    app.on_mouse_leave();

    let b = window_bounds(&app);
    assert!(
        (b.width - dragged.width).abs() <= 0.001,
        "the pointer leaving the window must not resize it (width went from {} to {})",
        dragged.width,
        b.width
    );
    assert!(
        (b.height - dragged.height).abs() <= 0.001,
        "the pointer leaving the window must not resize it (height went from {} to {})",
        dragged.height,
        b.height
    );
    assert!(
        (b.y - dragged.y).abs() <= 0.001,
        "the pointer leaving the window must not move it"
    );
}

// The minimum size clamp has to stop the edge being dragged, not slide the
// window: dragging the left handle right past the minimum leaves the right
// edge exactly where it was.
#[test]
fn left_grab_dragged_past_the_minimum_width_leaves_the_right_edge_alone() {
    let (mut app, mut mouse) = start_drag(H::Left, V::Stretch);
    let start = window_bounds(&app);
    let start_right = start.x + start.width;

    // far further right than the window is wide
    mouse = Point::new(mouse.x + start.width * 2.0, mouse.y);
    move_to(&mut app, mouse);

    let b = window_bounds(&app);
    assert!(
        (b.width - MIN_W).abs() <= 0.001,
        "the window should stop at its minimum width, was {}",
        b.width
    );
    assert!(
        (b.x + b.width - start_right).abs() <= 0.001,
        "the edge that is not being dragged must not move (right went from {start_right} to {})",
        b.x + b.width
    );
}

/// A container that zooms and offsets its children, as the geometry-node
/// editor's graph does (C#: `ParentToChildTransform = Scaling(zoom) *
/// Translation(40, 30)`, i.e. screen = local * zoom + (40, 30)).
struct Zoomed {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    zoom: f64,
}

impl Widget for Zoomed {
    fn type_name(&self) -> &'static str {
        "Zoomed"
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
        for child in &mut self.children {
            child.layout(available);
        }
        available
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn child_transform(&self) -> Option<TransAffine> {
        let mut t = TransAffine::new_scaling_uniform(self.zoom);
        t.translate(40.0, 30.0);
        Some(t)
    }
}

/// Drags the left or right edge of a window inside a container zoomed by
/// `zoom` `STEP_COUNT` steps of `STEP_SIZE` screen pixels outward and returns
/// (how far the dragged edge moved on screen, how far the other edge moved).
fn zoomed_edge_drag(zoom: f64, right: bool) -> (f64, f64) {
    const W: f64 = 1200.0;
    const HT: f64 = 900.0;
    let zoomed = Zoomed {
        bounds: Rect::new(0.0, 0.0, W, HT),
        children: vec![Box::new(new_window())],
        zoom,
    };
    let mut app = App::new(Box::new(zoomed));
    app.layout(Size::new(W, HT));
    let win = |app: &App| app.root().children()[0].bounds();
    let to_screen = |x: f64| x * zoom + 40.0;
    let edges = |b: Rect| {
        let (l, r) = (to_screen(b.x), to_screen(b.x + b.width));
        if right {
            (r, l)
        } else {
            (l, r)
        }
    };
    let start = win(&app);
    let (start_edge, start_other) = edges(start);
    let local = grab_local(start, if right { H::Right } else { H::Left }, V::Stretch);
    let mut mouse = Point::new(
        to_screen(start.x + local.x),
        (start.y + local.y) * zoom + 30.0,
    );
    app.on_mouse_move(mouse.x, screen_y(HT, mouse.y));
    app.on_mouse_down(
        mouse.x,
        screen_y(HT, mouse.y),
        MouseButton::Left,
        Modifiers::default(),
    );
    // outward, so the minimum size cannot stop the edge
    let step = if right { STEP_SIZE } else { -STEP_SIZE };
    for _ in 0..STEP_COUNT {
        mouse = Point::new(mouse.x + step, mouse.y);
        app.on_mouse_move(mouse.x, screen_y(HT, mouse.y));
    }
    app.on_mouse_up(
        mouse.x,
        screen_y(HT, mouse.y),
        MouseButton::Left,
        Modifiers::default(),
    );
    let (edge, other) = edges(win(&app));
    (edge - start_edge, other - start_other)
}

/// A window inside a zoomed, offset container is sized in the container's
/// units, so a handle drag has to be measured in those units: the dragged edge
/// stays under the mouse on screen, not grows by the mouse travel times the
/// zoom. C#'s four `[Arguments]` cases run in one test.
#[test]
fn an_edge_drag_in_a_zoomed_container_keeps_the_edge_under_the_mouse() {
    for (zoom, right) in [(0.5, true), (1.5, true), (0.5, false), (1.5, false)] {
        let edge = if right { "Right" } else { "Left" };
        let want = if right { STEP_SIZE } else { -STEP_SIZE } * STEP_COUNT as f64;
        let (moved, other_moved) = zoomed_edge_drag(zoom, right);
        assert!(
            (moved - want).abs() <= 0.5,
            "at zoom {zoom} the {edge} edge has to follow the mouse {want} px on screen, it moved {moved}"
        );
        assert!(
            other_moved.abs() <= 0.5,
            "at zoom {zoom} the edge that is not dragged must stay put, it moved {other_moved}"
        );
    }
}

#[test]
fn rust_only_a_half_unit_edge_drag_leaves_the_other_edge_alone() {
    // At zoom 2 every 5 px step is 2.5 window units: each odd step lands on a
    // half unit, where rounding the left and the width separately moved the
    // right edge.
    for right in [true, false] {
        let (_, other_moved) = zoomed_edge_drag(2.0, right);
        assert_eq!(other_moved, 0.0, "right={right}: the undragged edge moved");
    }
    // The per-step case, which the 10-step total above lands on whole units
    // for: one 5 px step left at zoom 2.
    let zoomed = Zoomed {
        bounds: Rect::new(0.0, 0.0, 1200.0, 900.0),
        children: vec![Box::new(new_window())],
        zoom: 2.0,
    };
    let mut app = App::new(Box::new(zoomed));
    app.layout(Size::new(1200.0, 900.0));
    let start = app.root().children()[0].bounds();
    let start_right = start.x + start.width;
    let x = (start.x + GRAB_MID) * 2.0 + 40.0;
    let y = 900.0 - ((start.y + start.height / 2.0) * 2.0 + 30.0);
    app.on_mouse_move(x, y);
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(x - STEP_SIZE, y);
    let b = app.root().children()[0].bounds();
    assert_eq!(
        b.x + b.width,
        start_right,
        "a half-unit drag of the left edge moved the right edge: {b:?}"
    );
    // 2.5 units of travel: the left edge rounds half away from zero, to 98.
    assert_eq!(b.x, start.x - 2.0, "the left edge rounds to a whole unit");
}
