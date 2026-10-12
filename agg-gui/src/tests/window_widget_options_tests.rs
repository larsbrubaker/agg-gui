//! Port of agg-sharp `WindowWidgetOptionsTests`: the runtime options of a
//! `Window` (title, resizable, collapsible, auto-size) change the live window.
//!
//! C#'s option setters map to agg-gui's runtime cells (`with_title_cell`,
//! `with_resizable_cell`, `with_collapsible_cell`, `with_auto_size_cell`),
//! which the window applies on its next `layout`, so each change here is
//! followed by a layout pass. agg-gui's grab handles are not child widgets:
//! "every grab control visible" is the eight handle zones hit-testing as
//! resize handles (`Window::resize_dir`) with `resize_handles_active` set.
//! C#'s `ClientArea` is the window's content child, so its height is the
//! window's height below the title bar, and the content C# adds to it sits in
//! a column that fills that area. The chevron is clicked through the window's
//! own pointer path (`InvokeClick` in C#).
//!
//! `DefaultsAreUnchanged` is skipped: it pins C#'s legacy defaults, which
//! agg-gui's `Window` does not share (collapsing is on by default here).

use crate::event::{Event, EventResult, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::window::Window;
use crate::{DrawCtx, FlexColumn, SizedBox};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

const PARENT: Size = Size::new(800.0, 600.0);
/// `Window`'s title bar height.
const TITLE_H: f64 = 28.0;

struct Cells {
    title: Rc<RefCell<String>>,
    resizable: Rc<Cell<bool>>,
    collapsible: Rc<Cell<bool>>,
    auto_size: Rc<Cell<bool>>,
}

fn build_with(content: Box<dyn Widget>) -> (Window, Cells) {
    let font = Arc::new(Font::from_slice(super::TEST_FONT).expect("test font"));
    let cells = Cells {
        title: Rc::new(RefCell::new("Original".to_string())),
        resizable: Rc::new(Cell::new(true)),
        collapsible: Rc::new(Cell::new(false)),
        auto_size: Rc::new(Cell::new(false)),
    };
    // C#: new RectangleDouble(100, 100, 400, 350), AddTitleBar("Original")
    let mut window = Window::new("Original", font, content)
        .with_bounds(Rect::new(100.0, 100.0, 300.0, 250.0))
        .with_title_cell(Rc::clone(&cells.title))
        .with_resizable_cell(Rc::clone(&cells.resizable))
        .with_collapsible_cell(Rc::clone(&cells.collapsible))
        .with_auto_size_cell(Rc::clone(&cells.auto_size));
    window.layout(PARENT);
    (window, cells)
}

fn build() -> (Window, Cells) {
    build_with(Box::new(SizedBox::new()))
}

/// The middle of each of the eight grab handles, window-local.
fn handle_points(window: &Window) -> [Point; 8] {
    let b = window.bounds();
    let (l, r, cx) = (3.0, b.width - 3.0, b.width / 2.0);
    let (bt, t, cy) = (3.0, b.height - 3.0, b.height / 2.0);
    [
        Point::new(l, bt),
        Point::new(l, t),
        Point::new(r, bt),
        Point::new(r, t),
        Point::new(l, cy),
        Point::new(r, cy),
        Point::new(cx, bt),
        Point::new(cx, t),
    ]
}

fn grab_controls_shown(window: &Window) -> bool {
    window.resize_handles_active()
        && handle_points(window)
            .iter()
            .all(|p| window.resize_dir(*p).is_some())
}

fn grab_controls_hidden(window: &Window) -> bool {
    !window.resize_handles_active()
        && handle_points(window)
            .iter()
            .all(|p| window.resize_dir(*p).is_none())
}

fn top(window: &Window) -> f64 {
    window.bounds().y + window.bounds().height
}

fn client_height(window: &Window) -> f64 {
    window.bounds().height - TITLE_H
}

/// Clicks the collapse chevron through the window's pointer path; the
/// window acts on the click in its next layout.
fn click_chevron(window: &mut Window) {
    let r = window.collapse_button_rect().expect("chevron shown");
    let pos = Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0);
    let mods = Modifiers::default();
    let button = MouseButton::Left;
    window.on_event(&Event::MouseDown {
        pos,
        button,
        modifiers: mods,
    });
    window.on_event(&Event::MouseUp {
        pos,
        button,
        modifiers: mods,
    });
    window.layout(PARENT);
}

#[test]
fn title_changes_the_title_bar_text() {
    let (mut window, cells) = build();
    *cells.title.borrow_mut() = "Renamed".to_string();
    window.layout(PARENT);
    assert_eq!(window.title_bar_text(), "Renamed");
    assert_ne!(window.title_bar_text(), "Original");
}

#[test]
fn resizable_toggles_the_grab_handles() {
    let (mut window, cells) = build();
    cells.resizable.set(false);
    window.layout(PARENT);
    assert!(grab_controls_hidden(&window));
    cells.resizable.set(true);
    window.layout(PARENT);
    assert!(grab_controls_shown(&window));
}

#[test]
fn collapsing_folds_to_the_title_bar_keeping_the_top_edge() {
    let (mut window, cells) = build();
    cells.collapsible.set(true);
    window.layout(PARENT);
    assert!(window.collapse_button_shown());

    let top_edge = top(&window);
    let height = window.bounds().height;
    click_chevron(&mut window);
    assert!(window.is_collapsed());
    // C#: ClientArea.Visible is false — the content area clips to nothing.
    let (_, _, _, clip_h) = window.clip_children_rect().expect("content clip");
    assert!(
        clip_h <= 0.0,
        "the content area must be hidden, clip height {clip_h}"
    );
    assert!(window.bounds().height < TITLE_H * 2.0);
    assert!((top(&window) - top_edge).abs() <= 0.001);
    assert!(grab_controls_hidden(&window));

    click_chevron(&mut window);
    assert!(!window.is_collapsed());
    assert!((window.bounds().height - height).abs() <= 0.001);
    assert!((top(&window) - top_edge).abs() <= 0.001);
    assert!(grab_controls_shown(&window));

    // No longer collapsible: the chevron goes and a folded window unfolds.
    window.set_collapsed(true);
    cells.collapsible.set(false);
    window.layout(PARENT);
    assert!(!window.collapse_button_shown());
    assert!(!window.is_collapsed());
    assert!((window.bounds().height - height).abs() <= 0.001);
}

/// A content child of a fixed width whose height a test can change, like
/// C#'s `new GuiWidget(100, 40) { VAnchor = Absolute | Top }`.
struct Content {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    height: Rc<Cell<f64>>,
}

impl Widget for Content {
    fn type_name(&self) -> &'static str {
        "Content"
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
    fn layout(&mut self, _: Size) -> Size {
        Size::new(100.0, self.height.get())
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

#[test]
fn auto_size_fits_the_height_to_the_content_and_follows_it() {
    let height = Rc::new(Cell::new(40.0));
    let content = Content {
        bounds: Rect::default(),
        children: Vec::new(),
        height: Rc::clone(&height),
    };
    let client_area = FlexColumn::new().add(Box::new(content));
    let (mut window, cells) = build_with(Box::new(client_area));
    let top_edge = top(&window);
    let width = window.bounds().width;

    cells.auto_size.set(true);
    window.layout(PARENT);
    assert!(
        (client_height(&window) - 40.0).abs() <= 0.5,
        "client area height {}",
        client_height(&window)
    );
    assert!((top(&window) - top_edge).abs() <= 0.001);
    assert_eq!(window.bounds().width, width);
    assert!(grab_controls_hidden(&window));

    height.set(120.0);
    window.layout(PARENT);
    assert!(
        (client_height(&window) - 120.0).abs() <= 0.5,
        "client area height {}",
        client_height(&window)
    );
    assert!((top(&window) - top_edge).abs() <= 0.001);

    cells.auto_size.set(false);
    window.layout(PARENT);
    assert!(grab_controls_shown(&window));
}
