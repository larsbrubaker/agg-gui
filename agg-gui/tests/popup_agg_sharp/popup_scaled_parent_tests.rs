//! agg-sharp `PopupScaledParentTests`: a popup opened from a widget inside a
//! zoomed parent (a card in the zoomed node editor) mates to where the anchor
//! is drawn. agg-gui's widgets anchor a popup by asking for their own
//! rectangle in root space while they handle the press
//! ([`event_rect_to_root`]) and placing a [`Popup`] there; these tests press
//! a widget under a zooming canvas through a real `App` and place the popup
//! the same way. C#'s `[Arguments]` rows run as loops; the window is
//! `800 x 600` logical units at device scale 1 and 2.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::{
    device_scale, event_rect_to_root, set_device_scale, Align2, App, DrawCtx, Event, EventResult,
    Modifiers, MouseButton, Point, Popup, Rect, RectAlign, Size, TransAffine, Widget,
};

const CANVAS_PAN: Point = Point::new(20.0, 10.0);
const ANCHOR_POSITION: Point = Point::new(100.0, 300.0);
const ROWS: [(f64, f64); 5] = [(1.0, 1.0), (0.5, 1.0), (1.5, 1.0), (0.5, 2.0), (1.5, 2.0)];

type Opened = Rc<RefCell<Option<Rect>>>;

/// A widget that, pressed, opens a 60 x 30 popup placed by `align` against
/// `anchor_local` (a rect in its own space) in root space.
struct Node {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    child_transform: Option<TransAffine>,
    opens: Option<(RectAlign, Rect, Opened)>,
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
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, e: &Event) -> EventResult {
        let (Event::MouseDown { .. }, Some((align, anchor_local, opened))) = (e, &self.opens)
        else {
            return EventResult::Ignored;
        };
        let mut popup = Popup::new();
        popup.align = *align;
        popup.gap = 0.0;
        popup.set_anchor(event_rect_to_root(*anchor_local).expect("inside a dispatch"));
        popup.set_size(Size::new(60.0, 30.0));
        *opened.borrow_mut() = Some(popup.rect(Size::new(800.0, 600.0)));
        EventResult::Consumed
    }
    fn child_transform(&self) -> Option<TransAffine> {
        self.child_transform
    }
}

/// window > canvas (zoom `scale`, pan 20, 10) > anchor (at 100, 300,
/// 40 x 20); presses the anchor's middle and returns the popup it opened.
fn open_from_anchor(scale: f64, device: f64, align: RectAlign, anchor_local: Rect) -> Rect {
    let opened: Opened = Rc::default();
    let anchor = Node {
        bounds: Rect::new(ANCHOR_POSITION.x, ANCHOR_POSITION.y, 40.0, 20.0),
        children: vec![],
        child_transform: None,
        opens: Some((align, anchor_local, Rc::clone(&opened))),
    };
    let mut zoom = TransAffine::new_scaling(scale, scale);
    zoom.multiply(&TransAffine::new_translation(CANVAS_PAN.x, CANVAS_PAN.y));
    let canvas = Node {
        bounds: Rect::new(0.0, 0.0, 1000.0, 1000.0),
        children: vec![Box::new(anchor)],
        child_transform: Some(zoom),
        opens: None,
    };
    let window = Node {
        bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
        children: vec![Box::new(canvas)],
        child_transform: None,
        opens: None,
    };

    let saved = device_scale();
    set_device_scale(device);
    let mut app = App::new(Box::new(window));
    app.layout(Size::new(800.0 * device, 600.0 * device));
    let middle = drawn(scale, Point::new(20.0, 10.0));
    let (x, y_down) = (middle.x * device, (600.0 - middle.y) * device);
    app.on_mouse_down(x, y_down, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y_down, MouseButton::Left, Modifiers::default());
    set_device_scale(saved);

    let popup = *opened.borrow();
    popup.expect("the press reaches the anchor")
}

/// Where a point in the anchor's coordinates is drawn, written out by hand.
fn drawn(scale: f64, anchor_point: Point) -> Point {
    Point::new(
        (ANCHOR_POSITION.x + anchor_point.x) * scale + CANVAS_PAN.x,
        (ANCHOR_POSITION.y + anchor_point.y) * scale + CANVAS_PAN.y,
    )
}

fn assert_near(actual: f64, expected: f64, why: &str) {
    assert!(
        (actual - expected).abs() <= 1e-9,
        "{why}: {actual} != {expected}"
    );
}

#[test]
fn a_popup_beside_the_anchor_mates_to_its_drawn_edges() {
    for (scale, device) in ROWS {
        // Beside and above, the way a sub menu opens: the anchor's right to
        // the popup's left, its top to the popup's bottom — both need the
        // anchor's drawn width and height.
        let align = RectAlign {
            parent: Align2::RIGHT_TOP,
            child: Align2::LEFT_BOTTOM,
        };
        let popup = open_from_anchor(scale, device, align, Rect::new(0.0, 0.0, 40.0, 20.0));
        let top_right = drawn(scale, Point::new(40.0, 20.0));
        let why = format!("zoom {scale}, device scale {device}");
        assert_near(
            popup.x,
            top_right.x,
            &format!("popup left at the anchor's drawn right, {why}"),
        );
        assert_near(
            popup.y,
            top_right.y,
            &format!("popup bottom at the anchor's drawn top, {why}"),
        );
    }
}

#[test]
fn a_popup_at_a_click_point_opens_where_the_click_is_drawn() {
    for (scale, device) in ROWS {
        // A context menu opens its top left corner at the click, given as a
        // point rectangle in the anchor.
        let click = Point::new(7.0, 3.0);
        let align = RectAlign {
            parent: Align2::LEFT_TOP,
            child: Align2::LEFT_TOP,
        };
        let popup = open_from_anchor(scale, device, align, Rect::new(click.x, click.y, 0.0, 0.0));
        let drawn_click = drawn(scale, click);
        let why = format!("zoom {scale}, device scale {device}");
        assert_near(
            popup.x,
            drawn_click.x,
            &format!("popup left at the drawn click, {why}"),
        );
        assert_near(
            popup.y + popup.height,
            drawn_click.y,
            &format!("popup top at the drawn click, {why}"),
        );
    }
}
