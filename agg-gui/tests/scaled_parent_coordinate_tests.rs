//! agg-sharp `ScaledParentCoordinateTests` (`Tests/Agg.Tests/Agg.UI`), ported
//! against `agg_gui::widget::coords`: the coordinate helpers have to agree
//! with where a widget is drawn under a parent that zooms its children (a
//! `child_transform`, the node editor's card layer), and "from" has to undo
//! "to". Same test names; C#'s `[Arguments]` rows run as loops.
//!
//! C# `ParentToChildTransform` is a widget's whole placement in its parent;
//! agg-gui splits it into the widget's `bounds()` origin and its parent's
//! `child_transform`. C#'s canvas (zoom, then pan, as its own transform)
//! becomes a canvas at the origin whose `child_transform` is that zoom and
//! pan, which draws its children in the same place. Every case runs at
//! device scale 1 and 2: the window is laid out at `800 x 600` logical
//! units, so presses arrive in device pixels.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widget::coords::{point_from, point_to, rect_from, rect_to, CoordSpace};
use agg_gui::{
    device_scale, set_device_scale, App, DrawCtx, Event, EventResult, Modifiers, MouseButton,
    Point, Rect, Size, TransAffine, Widget, Window,
};

const CONTAINER_POSITION: Point = Point::new(30.0, 40.0);
const CANVAS_PAN: Point = Point::new(20.0, 10.0);
const GROUP_POSITION: Point = Point::new(50.0, 60.0);
const CHILD_POSITION: Point = Point::new(100.0, 80.0);

/// Path from the window to the child in [`build_tree`]: container, canvas,
/// group, child.
const CHILD_PATH: [usize; 4] = [0, 0, 0, 0];
/// The canvas is at depth 2 (window, container, canvas).
const CANVAS_DEPTH: usize = 2;

/// A plain widget at `bounds`, optionally transforming its children, that
/// records where in its own space a press reached it.
struct Node {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    child_transform: Option<TransAffine>,
    received: Rc<Cell<Option<Point>>>,
}

impl Node {
    fn new(bounds: Rect, children: Vec<Box<dyn Widget>>) -> Self {
        Self {
            bounds,
            children,
            child_transform: None,
            received: Rc::default(),
        }
    }
}

impl Widget for Node {
    fn type_name(&self) -> &'static str {
        "Node"
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
    fn layout(&mut self, _available: Size) -> Size {
        for child in &mut self.children {
            let b = child.bounds();
            child.layout(Size::new(b.width, b.height));
        }
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, e: &Event) -> EventResult {
        if let Event::MouseDown { pos, .. } = e {
            self.received.set(Some(*pos));
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
    fn child_transform(&self) -> Option<TransAffine> {
        self.child_transform
    }
}

/// C# `Affine.NewScaling(sx, sy) * Affine.NewTranslation(tx, ty)`: scale,
/// then move.
fn scale_then_move(sx: f64, sy: f64, tx: f64, ty: f64) -> TransAffine {
    let mut t = TransAffine::new_scaling(sx, sy);
    t.multiply(&TransAffine::new_translation(tx, ty));
    t
}

fn at_device_scale(scale: f64, body: impl FnOnce()) {
    let saved = device_scale();
    set_device_scale(scale);
    body();
    set_device_scale(saved);
}

/// window > container (at 30, 40) > canvas (zoom `scale`, pan 20, 10) >
/// group (at 50, 60) > child (at 100, 80, 40 x 20). Returns the window and
/// the cell the child records presses in.
fn build_tree(canvas_transform: TransAffine) -> (Node, Rc<Cell<Option<Point>>>) {
    let child = Node::new(
        Rect::new(CHILD_POSITION.x, CHILD_POSITION.y, 40.0, 20.0),
        vec![],
    );
    let received = Rc::clone(&child.received);
    let group = Node::new(
        Rect::new(GROUP_POSITION.x, GROUP_POSITION.y, 400.0, 300.0),
        vec![Box::new(child)],
    );
    let mut canvas = Node::new(Rect::new(0.0, 0.0, 1000.0, 1000.0), vec![Box::new(group)]);
    canvas.child_transform = Some(canvas_transform);
    let container = Node::new(
        Rect::new(CONTAINER_POSITION.x, CONTAINER_POSITION.y, 700.0, 500.0),
        vec![Box::new(canvas)],
    );
    let window = Node::new(Rect::new(0.0, 0.0, 800.0, 600.0), vec![Box::new(container)]);
    (window, received)
}

fn zoomed_tree(scale: f64) -> (Node, Rc<Cell<Option<Point>>>) {
    build_tree(scale_then_move(scale, scale, CANVAS_PAN.x, CANVAS_PAN.y))
}

/// Where a point in the child's coordinates is drawn in the window, written
/// out by hand rather than through the helpers under test.
fn drawn_in_window(scale: f64, child_point: Point) -> Point {
    Point::new(
        CONTAINER_POSITION.x
            + (GROUP_POSITION.x + CHILD_POSITION.x + child_point.x) * scale
            + CANVAS_PAN.x,
        CONTAINER_POSITION.y
            + (GROUP_POSITION.y + CHILD_POSITION.y + child_point.y) * scale
            + CANVAS_PAN.y,
    )
}

fn drawn_in_window_y(scale_y: f64, child_y: f64) -> f64 {
    CONTAINER_POSITION.y + (GROUP_POSITION.y + CHILD_POSITION.y + child_y) * scale_y + CANVAS_PAN.y
}

const WINDOW: CoordSpace = CoordSpace::Ancestor(0);

fn assert_near(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= 1e-9,
        "{what}: {actual} != {expected}"
    );
}

fn assert_point_near(actual: Point, expected: Point, what: &str) {
    assert_near(actual.x, expected.x, &format!("{what} x"));
    assert_near(actual.y, expected.y, &format!("{what} y"));
}

fn assert_rect_near(actual: Rect, expected: Rect, what: &str) {
    assert_near(actual.x, expected.x, &format!("{what} left"));
    assert_near(actual.y, expected.y, &format!("{what} bottom"));
    assert_near(
        actual.x + actual.width,
        expected.x + expected.width,
        &format!("{what} right"),
    );
    assert_near(
        actual.y + actual.height,
        expected.y + expected.height,
        &format!("{what} top"),
    );
}

/// The rectangle from corner `a` to corner `b`.
fn corners(a: Point, b: Point) -> Rect {
    Rect::new(a.x, a.y, b.x - a.x, b.y - a.y)
}

const ZOOMED_ROWS: [(f64, f64); 4] = [(0.5, 1.0), (1.5, 1.0), (0.5, 2.0), (1.5, 2.0)];

#[test]
fn point_helpers_agree_with_where_the_child_is_drawn() {
    for (scale, device) in ZOOMED_ROWS {
        at_device_scale(device, || {
            let (window, received) = zoomed_tree(scale);
            let child_point = Point::new(7.0, 3.0);
            let drawn = drawn_in_window(scale, child_point);

            let to_window = point_to(&window, &CHILD_PATH, WINDOW, child_point).unwrap();
            assert_point_near(to_window, drawn, "to window");
            let to_screen =
                point_to(&window, &CHILD_PATH, CoordSpace::Screen, child_point).unwrap();
            assert_point_near(to_screen, drawn, "to screen");

            let from_window = point_from(&window, &CHILD_PATH, WINDOW, drawn).unwrap();
            assert_point_near(from_window, child_point, "from window");
            let from_screen = point_from(&window, &CHILD_PATH, CoordSpace::Screen, drawn).unwrap();
            assert_point_near(from_screen, child_point, "from screen");

            // The mouse routing is a separate walk (each child inverts its
            // parent's transform); a press where the helpers say the point is
            // drawn has to land on the child at that point.
            let mut app = App::new(Box::new(window));
            app.layout(Size::new(800.0 * device, 600.0 * device));
            let (x, y_down) = (to_screen.x * device, (600.0 - to_screen.y) * device);
            app.on_mouse_down(x, y_down, MouseButton::Left, Modifiers::default());
            app.on_mouse_up(x, y_down, MouseButton::Left, Modifiers::default());
            let got = received.get().expect("the press reaches the child");
            assert_point_near(got, child_point, "press");
        });
    }
}

#[test]
fn rectangle_helpers_agree_with_where_the_child_is_drawn() {
    for (scale, device) in ZOOMED_ROWS {
        at_device_scale(device, || {
            let (window, _) = zoomed_tree(scale);
            let local = Rect::new(0.0, 0.0, 40.0, 20.0);
            let drawn_rect = corners(
                drawn_in_window(scale, Point::new(0.0, 0.0)),
                drawn_in_window(scale, Point::new(40.0, 20.0)),
            );

            for space in [WINDOW, CoordSpace::Screen] {
                let drawn = rect_to(&window, &CHILD_PATH, space, local).unwrap();
                assert_rect_near(drawn, drawn_rect, "to");
                let back = rect_from(&window, &CHILD_PATH, space, drawn_rect).unwrap();
                assert_rect_near(back, local, "from");
            }
        });
    }
}

#[test]
fn to_then_from_round_trips_to_the_input() {
    for (scale, device) in ZOOMED_ROWS {
        at_device_scale(device, || {
            let (window, _) = zoomed_tree(scale);
            let point = Point::new(3.25, -11.5);
            let rectangle = corners(Point::new(-4.0, 2.5), Point::new(13.0, 9.0));

            // C#'s window, canvas, canvas.Parent and null (the screen).
            for space in [
                WINDOW,
                CoordSpace::Ancestor(CANVAS_DEPTH),
                CoordSpace::Ancestor(CANVAS_DEPTH - 1),
                CoordSpace::Screen,
            ] {
                let there = point_to(&window, &CHILD_PATH, space, point).unwrap();
                let point2 = point_from(&window, &CHILD_PATH, space, there).unwrap();
                assert_point_near(point2, point, "point round trip");

                let there = rect_to(&window, &CHILD_PATH, space, rectangle).unwrap();
                let rectangle2 = rect_from(&window, &CHILD_PATH, space, there).unwrap();
                assert_rect_near(rectangle2, rectangle, "rectangle round trip");
            }
        });
    }
}

/// With no zoom anywhere the helpers are plain offsets, bit for bit: "to"
/// adds each translation in turn from the child up and "from" subtracts them
/// from the top down.
#[test]
fn a_pure_translation_tree_is_exact_offsets() {
    for device in [1.0, 2.0] {
        at_device_scale(device, || {
            let (window, _) = build_tree(TransAffine::new_translation(0.1, 0.7));
            let point = Point::new(0.3, 0.2);

            // Child, group, canvas (its child transform), container.
            let offsets = [
                CHILD_POSITION,
                GROUP_POSITION,
                Point::new(0.1, 0.7),
                CONTAINER_POSITION,
            ];
            let expected_to = offsets
                .iter()
                .fold(point, |p, o| Point::new(p.x + o.x, p.y + o.y));
            for space in [WINDOW, CoordSpace::Screen] {
                let to = point_to(&window, &CHILD_PATH, space, point).unwrap();
                assert_eq!(to.x, expected_to.x);
                assert_eq!(to.y, expected_to.y);
            }

            // agg-gui's `Rect` keeps a width, not a right edge, so the left
            // and bottom edges are the bit-exact offsets; right and top are
            // `x + width` and are checked to the same 1e-9 as elsewhere.
            let rectangle = corners(Point::new(0.3, 0.2), Point::new(1.1, 2.9));
            let expected_rect = offsets.iter().fold(rectangle, |r, o| {
                Rect::new(r.x + o.x, r.y + o.y, r.width, r.height)
            });
            let to_rect = rect_to(&window, &CHILD_PATH, WINDOW, rectangle).unwrap();
            assert_eq!(to_rect.x, expected_rect.x);
            assert_eq!(to_rect.y, expected_rect.y);
            assert_rect_near(to_rect, expected_rect, "rectangle");

            let screen = Point::new(123.4, 56.7);
            let expected_from = Point::new(
                screen.x - CONTAINER_POSITION.x - 0.1 - GROUP_POSITION.x - CHILD_POSITION.x,
                screen.y - CONTAINER_POSITION.y - 0.7 - GROUP_POSITION.y - CHILD_POSITION.y,
            );
            for space in [WINDOW, CoordSpace::Screen] {
                let from = point_from(&window, &CHILD_PATH, space, screen).unwrap();
                assert_eq!(from.x, expected_from.x);
                assert_eq!(from.y, expected_from.y);
            }
        });
    }
}

/// "From" lands in the widget's own coordinates: it undoes the widget's own
/// offset as well as every ancestor's. (C# also covers a widget whose
/// `LocalBounds` starts below zero, a `TextWidget`'s descent; an agg-gui
/// widget's local space always starts at its own origin, so here both the
/// widget and its parent sit away from their parents' origins instead.)
#[test]
fn from_lands_in_the_widgets_own_coordinates_when_its_bounds_are_offset() {
    for device in [1.0, 2.0] {
        at_device_scale(device, || {
            let text = Node::new(Rect::new(33.0, 21.0, 60.0, 16.0), vec![]);
            let offset_parent =
                Node::new(Rect::new(120.0, 90.0, 300.0, 200.0), vec![Box::new(text)]);
            let window = Node::new(
                Rect::new(0.0, 0.0, 800.0, 600.0),
                vec![Box::new(offset_parent)],
            );
            let text_path = [0, 0];

            // Where the text's own point is drawn, written out by hand: each
            // widget offsets by its origin.
            let text_point = Point::new(2.0, 1.0);
            let drawn = Point::new(120.0 + 33.0 + text_point.x, 90.0 + 21.0 + text_point.y);
            for space in [WINDOW, CoordSpace::Screen] {
                let from = point_from(&window, &text_path, space, drawn).unwrap();
                assert_point_near(from, text_point, "from");
            }

            let parent_point = Point::new(-12.5, -6.0);
            let into_parent = point_from(
                &window,
                &[0],
                WINDOW,
                Point::new(120.0 + parent_point.x, 90.0 + parent_point.y),
            )
            .unwrap();
            assert_point_near(into_parent, parent_point, "from into parent");

            let there = point_to(&window, &text_path, WINDOW, text_point).unwrap();
            let round_trip = point_from(&window, &text_path, WINDOW, there).unwrap();
            assert_point_near(round_trip, text_point, "round trip");

            let rectangle = corners(Point::new(0.0, 0.0), Point::new(10.0, 5.0));
            let there = rect_to(&window, &text_path, CoordSpace::Screen, rectangle).unwrap();
            let back = rect_from(&window, &text_path, CoordSpace::Screen, there).unwrap();
            assert_rect_near(back, rectangle, "rectangle round trip");
        });
    }
}

static REGULAR: &[u8] = include_bytes!("../../demo/assets/Nunito_Regular.ttf");

/// A dialog is a floating [`Window`] (its content a child offset below the
/// title bar) inside the root. "From screen" has to undo every offset "to
/// screen" applied, the window's and its content's included.
#[test]
fn from_screen_undoes_to_screen_inside_a_nested_dialog() {
    for device in [1.0, 2.0] {
        at_device_scale(device, || {
            let font = Arc::new(Font::from_slice(REGULAR).expect("demo font"));
            let child = Node::new(Rect::new(25.0, 35.0, 60.0, 30.0), vec![]);
            let client = Node::new(Rect::new(0.0, 0.0, 300.0, 200.0), vec![Box::new(child)]);
            let movable = Window::new("Dialog", font, Box::new(client))
                .with_bounds(Rect::new(170.0, 130.0, 300.0, 230.0));
            let mut app = App::new(Box::new(Node::new(
                Rect::new(0.0, 0.0, 800.0, 600.0),
                vec![Box::new(movable)],
            )));
            app.layout(Size::new(800.0 * device, 600.0 * device));
            let root = app.root();
            let client_path = [0, 0];
            let child_path = [0, 0, 0];

            let client_origin =
                point_to(root, &client_path, CoordSpace::Screen, Point::new(0.0, 0.0)).unwrap();
            assert!(
                client_origin.x.hypot(client_origin.y) > 0.0,
                "the case needs the client away from the screen origin"
            );

            let point = Point::new(4.5, 7.25);
            let screen = point_to(root, &child_path, CoordSpace::Screen, point).unwrap();
            let to_root = point_to(root, &child_path, WINDOW, point).unwrap();
            assert_point_near(screen, to_root, "to screen");

            let back = point_from(root, &child_path, CoordSpace::Screen, screen).unwrap();
            assert_point_near(back, point, "back");

            let rectangle = corners(Point::new(1.0, 2.0), Point::new(13.0, 17.0));
            let there = rect_to(root, &child_path, CoordSpace::Screen, rectangle).unwrap();
            let rectangle_back = rect_from(root, &child_path, CoordSpace::Screen, there).unwrap();
            assert_rect_near(rectangle_back, rectangle, "rectangle back");
        });
    }
}

/// An unbounded coordinate under a shrinking ancestor is infinite, and a full
/// matrix multiply turns infinity times a zero term into NaN on the other
/// axis. The point helpers keep it on its own axis.
#[test]
fn an_infinite_coordinate_stays_on_its_own_axis() {
    for device in [1.0, 2.0] {
        at_device_scale(device, || {
            let (window, _) = zoomed_tree(0.5);

            let to =
                point_to(&window, &CHILD_PATH, WINDOW, Point::new(f64::INFINITY, 3.0)).unwrap();
            assert_eq!(to.x, f64::INFINITY);
            assert_near(to.y, drawn_in_window(0.5, Point::new(0.0, 3.0)).y, "to y");

            let to_screen = point_to(
                &window,
                &CHILD_PATH,
                CoordSpace::Screen,
                Point::new(7.0, f64::NEG_INFINITY),
            )
            .unwrap();
            assert_near(
                to_screen.x,
                drawn_in_window(0.5, Point::new(7.0, 0.0)).x,
                "to screen x",
            );
            assert_eq!(to_screen.y, f64::NEG_INFINITY);

            let from = point_from(
                &window,
                &CHILD_PATH,
                WINDOW,
                Point::new(
                    f64::NEG_INFINITY,
                    drawn_in_window(0.5, Point::new(0.0, 3.0)).y,
                ),
            )
            .unwrap();
            assert_eq!(from.x, f64::NEG_INFINITY);
            assert_near(from.y, 3.0, "from y");

            let from_screen = point_from(
                &window,
                &CHILD_PATH,
                CoordSpace::Screen,
                Point::new(drawn_in_window(0.5, Point::new(7.0, 0.0)).x, f64::INFINITY),
            )
            .unwrap();
            assert_near(from_screen.x, 7.0, "from screen x");
            assert_eq!(from_screen.y, f64::INFINITY);
        });
    }
}

/// A widget collapsed to zero scale has no inverse. Every point on a
/// collapsed axis is drawn in the same place, so "from" falls back to
/// removing the translation on that axis — finite — while the other axis is
/// still undone exactly. "To" of an infinite coordinate on a collapsed axis
/// is the collapsed position, not NaN.
#[test]
fn a_zero_scale_stays_finite() {
    for device in [1.0, 2.0] {
        at_device_scale(device, || {
            let (window, _) = build_tree(scale_then_move(0.0, 2.0, CANVAS_PAN.x, CANVAS_PAN.y));

            let from = point_from(&window, &CHILD_PATH, WINDOW, Point::new(400.0, 300.0)).unwrap();
            assert!(from.x.is_finite());
            assert!(from.y.is_finite());
            let expected_y = (300.0 - CONTAINER_POSITION.y - CANVAS_PAN.y) / 2.0
                - GROUP_POSITION.y
                - CHILD_POSITION.y;
            assert_near(from.y, expected_y, "from y");

            let from_screen = point_from(
                &window,
                &CHILD_PATH,
                CoordSpace::Screen,
                Point::new(400.0, 300.0),
            )
            .unwrap();
            assert_eq!(from_screen.x, from.x);
            assert_eq!(from_screen.y, from.y);

            let to =
                point_to(&window, &CHILD_PATH, WINDOW, Point::new(f64::INFINITY, 5.0)).unwrap();
            assert_near(to.x, CONTAINER_POSITION.x + CANVAS_PAN.x, "to x");
            assert_near(to.y, drawn_in_window_y(2.0, 5.0), "to y");

            // A sheared, singular transform (both rows the same) falls back
            // the same way on both axes.
            let (window, _) = build_tree(TransAffine::new_custom(
                1.0,
                1.0,
                1.0,
                1.0,
                CANVAS_PAN.x,
                CANVAS_PAN.y,
            ));
            let singular =
                point_from(&window, &CHILD_PATH, WINDOW, Point::new(400.0, 300.0)).unwrap();
            assert!(singular.x.is_finite());
            assert!(singular.y.is_finite());
        });
    }
}
