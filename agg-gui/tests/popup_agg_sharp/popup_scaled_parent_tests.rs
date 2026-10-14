//! agg-sharp `PopupScaledParentTests`: a popup opened from a widget inside a
//! zoomed parent (a card in the zoomed node editor) mates to where the anchor
//! is drawn. agg-gui's widgets anchor a popup by asking for their own
//! rectangle in root space while they handle the press
//! ([`event_rect_to_root`]) and placing a [`Popup`] there; these tests press
//! a widget under a zooming canvas through a real `App` and place the popup
//! the same way. C#'s `[Arguments]` rows run as loops; the window is
//! `800 x 600` logical units at device scale 1 and 2.
//!
//! The drop-down list tests open a [`ComboBox`] (C#'s `DropDownList`) by a
//! press through the `App`, paint one frame (where the list is placed) and
//! read where it opened with [`ComboBox::open_list_rect`], in root logical
//! units: C# reads the `PopupWidget` the click added to the window.

use std::cell::RefCell;
use std::rc::Rc;

use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    device_scale, event_rect_to_root, set_device_scale, Align2, App, ComboBox, DrawCtx, Event,
    EventResult, Framebuffer, GfxCtx, Modifiers, MouseButton, Point, Popup, Rect, RectAlign, Size,
    TransAffine, Widget,
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

// ── Drop-down lists ─────────────────────────────────────────────────────────

static FONT: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT).expect("test font"))
}

/// A drop-down sized to its widest text (C#'s `DropDownList` is
/// `HAnchor.Fit`), laid out at its parent's origin.
fn fitted(combo: ComboBox) -> ComboBox {
    let mut combo = combo.with_fit_width(true);
    combo.layout(Size::new(1000.0, 100.0));
    combo
}

/// window > canvas (zoom `scale`, pan 20, 10) > anchor (at `anchor_at`,
/// 40 x 20) > `combo` at the anchor's origin, in an `App` at `device`.
fn zoomed_app(scale: f64, device: f64, anchor_at: Point, combo: ComboBox) -> App {
    let anchor = Node {
        bounds: Rect::new(anchor_at.x, anchor_at.y, 40.0, 20.0),
        children: vec![Box::new(combo)],
        child_transform: None,
        opens: None,
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
    let mut app = App::new(Box::new(window));
    app.layout(Size::new(800.0 * device, 600.0 * device));
    app
}

/// Where a point in the anchor's coordinates (anchored at `at`) is drawn.
fn drawn_at(scale: f64, at: Point, p: Point) -> Point {
    Point::new(
        (at.x + p.x) * scale + CANVAS_PAN.x,
        (at.y + p.y) * scale + CANVAS_PAN.y,
    )
}

/// Press and release at `p` (root logical, Y-up) in a `800 x 600` window.
fn click_root(app: &mut App, device: f64, p: Point) {
    let (x, y_down) = (p.x * device, (600.0 - p.y) * device);
    app.on_mouse_down(x, y_down, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y_down, MouseButton::Left, Modifiers::default());
}

/// Paint one frame, which places an open list.
fn paint(app: &mut App, device: f64, window: Size) {
    let mut fb = Framebuffer::new(
        (window.width * device) as u32,
        (window.height * device) as u32,
    );
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

/// The combo at the end of `path` (child indices from the root).
fn combo_at<'a>(app: &'a App, path: &[usize]) -> &'a ComboBox {
    let mut w: &dyn Widget = app.root();
    for &i in path {
        w = w.children()[i].as_ref();
    }
    w.as_any()
        .and_then(|a| a.downcast_ref::<ComboBox>())
        .expect("a ComboBox")
}

const ZOOMED_COMBO: [usize; 3] = [0, 0, 0];

/// The combo's drawn bounds (its local `0, 0, w, h` through the zoom).
fn drawn_combo(app: &App, scale: f64, at: Point) -> Rect {
    let b = combo_at(app, &ZOOMED_COMBO).bounds();
    let low = drawn_at(scale, at, Point::new(0.0, 0.0));
    let high = drawn_at(scale, at, Point::new(b.width, b.height));
    Rect::new(low.x, low.y, high.x - low.x, high.y - low.y)
}

/// An anchor whose drawn edges do not start at its origin (C#: `LocalBounds`
/// of `-6, -4, 34, 16`, a text widget's bounds reaching below its origin by
/// the font's descent) is met at its drawn edges by both popup paths: a
/// mated popup and a drop-down list.
///
/// An agg-gui widget's local space always starts at its origin, so the
/// offset reaches each path the way it can in agg-gui: the mated popup is
/// anchored to the offset rectangle in the anchor's space, and the drop-down
/// list's box (C# gives `PopupLayoutEngine` the offset anchor directly) is a
/// [`ComboBox`] sitting at `-6, -4` in the anchor.
#[test]
fn both_popup_paths_meet_the_drawn_edges_of_an_anchor_not_starting_at_its_origin() {
    let rows = [(1.0, 1.0), (1.5, 1.0), (1.0, 2.0), (1.5, 2.0)];
    // C#'s `RectangleDouble(-6, -4, 34, 16)`: left, bottom, right, top.
    let offset_bounds = Rect::new(-6.0, -4.0, 40.0, 20.0);
    for (scale, device) in rows {
        let why = format!("zoom {scale}, device scale {device}");
        let drawn_bottom_left = drawn(scale, Point::new(offset_bounds.x, offset_bounds.y));

        // A mated popup: under the anchor, left edges together.
        let align = RectAlign {
            parent: Align2::LEFT_BOTTOM,
            child: Align2::LEFT_TOP,
        };
        let popup = open_from_anchor(scale, device, align, offset_bounds);
        assert_near(
            popup.x,
            drawn_bottom_left.x,
            &format!("a mated popup has to line up with the anchor's drawn left, {why}"),
        );
        assert_near(
            popup.y + popup.height,
            drawn_bottom_left.y,
            &format!("a mated popup has to meet the anchor's drawn bottom, {why}"),
        );

        // The drop-down list path, its box at the same offset in the anchor.
        let saved = device_scale();
        set_device_scale(device);
        let mut combo =
            fitted(ComboBox::new(vec!["Item 0", "Item 1"], 0, font()).with_no_selection("none"));
        let b = combo.bounds();
        combo.set_bounds(Rect::new(
            offset_bounds.x,
            offset_bounds.y,
            b.width,
            b.height,
        ));
        let mut app = zoomed_app(scale, device, ANCHOR_POSITION, combo);
        click_root(
            &mut app,
            device,
            drawn_at(scale, ANCHOR_POSITION, Point::new(10.0, 12.0)),
        );
        paint(&mut app, device, Size::new(800.0, 600.0));
        set_device_scale(saved);

        let list = combo_at(&app, &ZOOMED_COMBO)
            .open_list_rect()
            .expect("the click opens the list");
        assert_near(
            list.x,
            drawn_bottom_left.x,
            &format!("a drop-down list has to line up with the anchor's drawn left, {why}"),
        );
        assert_near(
            list.y + list.height,
            drawn_bottom_left.y,
            &format!("a drop-down list has to open right under its drawn bottom, {why}"),
        );
    }
}

#[test]
fn a_drop_down_list_opens_under_its_drawn_control() {
    for (scale, device) in ROWS {
        let saved = device_scale();
        set_device_scale(device);
        let combo =
            fitted(ComboBox::new(vec!["Item 0", "Item 1"], 0, font()).with_no_selection("none"));
        let mut app = zoomed_app(scale, device, ANCHOR_POSITION, combo);
        click_root(
            &mut app,
            device,
            drawn_at(scale, ANCHOR_POSITION, Point::new(10.0, 12.0)),
        );
        paint(&mut app, device, Size::new(800.0, 600.0));
        set_device_scale(saved);

        let drawn = drawn_combo(&app, scale, ANCHOR_POSITION);
        let list = combo_at(&app, &ZOOMED_COMBO)
            .open_list_rect()
            .expect("the click opens the list");
        let why = format!("zoom {scale}, device scale {device}");
        assert_near(
            list.x,
            drawn.x,
            &format!("the list has to open aligned with the drawn control, {why}"),
        );
        assert_near(
            list.y + list.height,
            drawn.y,
            &format!("the list has to open right under the drawn control, {why}"),
        );
    }
}

#[test]
fn a_right_aligned_drop_down_list_opening_up_meets_its_drawn_top_and_right() {
    let rows = [(0.5, 1.0), (1.5, 1.0), (0.5, 2.0), (1.5, 2.0)];
    for (scale, device) in rows {
        let saved = device_scale();
        set_device_scale(device);
        let combo = fitted(
            ComboBox::new(vec!["0", "1"], 0, font())
                .with_no_selection("a long label to be wider than its items")
                .with_opens_upward(true)
                .with_align_to_right_edge(true),
        );
        // Where there is room on the window both above it and to its left,
        // so neither choice is overruled.
        let at = Point::new(400.0, 150.0);
        let mut app = zoomed_app(scale, device, at, combo);
        click_root(
            &mut app,
            device,
            drawn_at(scale, at, Point::new(10.0, 12.0)),
        );
        paint(&mut app, device, Size::new(800.0, 600.0));
        set_device_scale(saved);

        let drawn = drawn_combo(&app, scale, at);
        let list = combo_at(&app, &ZOOMED_COMBO)
            .open_list_rect()
            .expect("the click opens the list");
        let why = format!("zoom {scale}, device scale {device}");
        assert_near(
            list.x + list.width,
            drawn.x + drawn.width,
            &format!("a right aligned list has to line up with the drawn control's right, {why}"),
        );
        assert_near(
            list.y,
            drawn.y + drawn.height,
            &format!("a list opening up has to sit on the drawn control's top, {why}"),
        );
    }
}

/// A drop down opened on an item deep in a list longer than its popup scrolls
/// that item to the popup's middle, so the entries either side of the
/// current one show too.
#[test]
fn opening_a_drop_down_centres_the_selected_item() {
    for (device, selected) in [(1.0, 5), (1.0, 20), (1.0, 34), (2.0, 20)] {
        let saved = device_scale();
        set_device_scale(device);
        let items: Vec<String> = (0..40).map(|i| format!("Item {i}")).collect();
        let mut combo = fitted(ComboBox::new(items, selected, font()).with_max_height(150.0));
        let b = combo.bounds();
        combo.set_bounds(Rect::new(10.0, 400.0, b.width, b.height));
        let window = Node {
            bounds: Rect::new(0.0, 0.0, 400.0, 600.0),
            children: vec![Box::new(combo)],
            child_transform: None,
            opens: None,
        };
        let mut app = App::new(Box::new(window));
        app.layout(Size::new(400.0 * device, 600.0 * device));
        let (x, y_down) = (20.0 * device, (600.0 - 412.0) * device);
        app.on_mouse_down(x, y_down, MouseButton::Left, Modifiers::default());
        app.on_mouse_up(x, y_down, MouseButton::Left, Modifiers::default());
        paint(&mut app, device, Size::new(400.0, 600.0));
        set_device_scale(saved);

        let combo = combo_at(&app, &[0]);
        let visible = combo.open_list_rect().expect("the click opens the list");
        let first = combo.open_list_item_rect(0).expect("row 0");
        let last = combo.open_list_item_rect(39).expect("row 39");
        let scroll_area = first.y + first.height - last.y;
        assert!(
            visible.height < scroll_area,
            "the list has to be longer than its popup for the scroll to matter"
        );

        let item = combo
            .open_list_item_rect(selected)
            .expect("the selected row");
        assert!(
            item.y >= visible.y - 1e-9 && item.y + item.height <= visible.y + visible.height + 1e-9,
            "item {selected} has to be visible when the list opens: at {item:?}, popup {visible:?}"
        );

        // Near either end of the list the scroll is clamped, so only an item
        // in the middle can sit centred.
        if selected == 20 {
            let (item_c, visible_c) =
                (item.y + item.height * 0.5, visible.y + visible.height * 0.5);
            assert!(
                (item_c - visible_c).abs() <= 1e-6,
                "item {selected} has to open in the middle of the popup: {item_c} != {visible_c}"
            );
        }
    }
}

/// Rust-only: a row of a list opened from a zoomed drop-down is picked where
/// it is drawn, and automation finds it there: the list paints at the
/// window's scale, so its rows are hit-tested in the same space.
#[test]
fn rust_only_a_zoomed_drop_down_picks_the_row_drawn_under_the_press() {
    for (scale, device) in ROWS {
        let saved = device_scale();
        set_device_scale(device);
        let combo = fitted(ComboBox::new(vec!["Item 0", "Item 1", "Item 2"], 0, font()));
        let mut app = zoomed_app(scale, device, ANCHOR_POSITION, combo);
        click_root(
            &mut app,
            device,
            drawn_at(scale, ANCHOR_POSITION, Point::new(10.0, 12.0)),
        );
        paint(&mut app, device, Size::new(800.0, 600.0));

        let combo = combo_at(&app, &ZOOMED_COMBO);
        let row = combo.open_list_item_rect(2).expect("row 2");
        // Automation's target for the row, in the combo's own space, is
        // drawn where the row is painted.
        let mut targets = Vec::new();
        combo.find_named_targets("Item 2 Menu Item", &mut targets);
        let local = targets.first().expect("row 2 is named").rect_local;
        let low = drawn_at(scale, ANCHOR_POSITION, Point::new(local.x, local.y));
        let why = format!("zoom {scale}, device scale {device}");
        assert_near(low.x, row.x, &format!("named row left, {why}"));
        assert_near(low.y, row.y, &format!("named row bottom, {why}"));
        assert_near(
            local.height * scale,
            row.height,
            &format!("named row height, {why}"),
        );

        click_root(
            &mut app,
            device,
            Point::new(row.x + 5.0, row.y + row.height * 0.5),
        );
        set_device_scale(saved);
        let combo = combo_at(&app, &ZOOMED_COMBO);
        assert_eq!(combo.selected(), 2, "the drawn row 2 is picked, {why}");
        assert!(!combo.is_open(), "picking a row closes the list, {why}");
    }
}
