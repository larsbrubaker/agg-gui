//! Regression tests for the auto-sized Window Resize Test sub-window.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    find_widget_by_id, App, Font, Modifiers, MouseButton, Point, Rect, Size, Stack, Widget, Window,
};
use demo_ui::{window_resize_sub_windows, ResizeTestWindow};

const CANVAS_W: f64 = 1280.0;
const CANVAS_H: f64 = 720.0;

fn font() -> Arc<Font> {
    const BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"))
}

fn make_w1_app() -> (App, String) {
    let entries: Vec<ResizeTestWindow> = window_resize_sub_windows(font());
    let entry = entries.into_iter().next().expect("W1 exists");
    let title = entry.title.clone();
    let visible = Rc::new(Cell::new(true));
    let mut win = Window::new(&title, font(), entry.content)
        .with_bounds(entry.initial_rect)
        .with_visible_cell(visible);
    if entry.auto_size {
        win = win.with_auto_size(true);
    }
    let root = Stack::new().add(Box::new(win));
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(CANVAS_W, CANVAS_H));
    (app, title)
}

fn to_screen(y_up: f64) -> f64 {
    CANVAS_H - y_up
}

fn screen_bounds_by_type(widget: &dyn Widget, type_name: &str, origin: Point) -> Option<Rect> {
    let b = widget.bounds();
    let here = Point::new(origin.x + b.x, origin.y + b.y);
    if widget.type_name() == type_name {
        return Some(Rect::new(here.x, here.y, b.width, b.height));
    }
    for child in widget.children() {
        if let Some(found) = screen_bounds_by_type(child.as_ref(), type_name, here) {
            return Some(found);
        }
    }
    None
}

fn drag(app: &mut App, start: (f64, f64), end: (f64, f64)) {
    app.on_mouse_move(start.0, start.1);
    app.on_mouse_down(start.0, start.1, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(end.0, end.1);
    app.on_mouse_up(end.0, end.1, MouseButton::Left, Modifiers::default());
    app.layout(Size::new(CANVAS_W, CANVAS_H));
}

fn assert_resize_right_gap(app: &App, title: &str) {
    let win = find_widget_by_id(app.root(), title)
        .expect("auto-sized window is present")
        .bounds();
    let resize =
        screen_bounds_by_type(app.root(), "Resize", Point::ORIGIN).expect("W1 contains Resize");
    let gap = (win.x + win.width) - (resize.x + resize.width);
    assert!(
        (gap - 10.0).abs() < 1.0,
        "outer window right edge should sit 10 px past inner Resize right edge; \
         win={win:?} resize={resize:?} gap={gap}"
    );
}

#[test]
fn w1_auto_sized_window_right_edge_tracks_inner_resize_after_drag() {
    let (mut app, title) = make_w1_app();
    assert_resize_right_gap(&app, &title);

    let resize =
        screen_bounds_by_type(app.root(), "Resize", Point::ORIGIN).expect("W1 contains Resize");
    let start = (resize.x + resize.width - 3.0, to_screen(resize.y + 3.0));
    drag(&mut app, start, (start.0 + 420.0, start.1));
    assert_resize_right_gap(&app, &title);

    let resize =
        screen_bounds_by_type(app.root(), "Resize", Point::ORIGIN).expect("W1 contains Resize");
    let start = (resize.x + resize.width - 3.0, to_screen(resize.y + 3.0));
    drag(&mut app, start, (start.0 - 420.0, start.1));
    assert_resize_right_gap(&app, &title);
}
