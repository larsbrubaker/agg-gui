//! Rust-only checks of the headless driver pieces the ported tests stand on:
//! [`HeadlessWindow`] input in C#'s Y-up window coordinates, frames on the
//! virtual clock, [`ProbeWidget`] event logs, and [`tree_query`]'s handles,
//! rectangles, clipping and `ActuallyVisibleOnScreen`.

use std::time::Duration;

use agg_gui::event::{Event, MouseButton};
use agg_gui::{Color, Point, Rect, Size};
use agg_gui_automation::driver::FRAME_INTERVAL;
use agg_gui_automation::tree_query::{
    self, actually_visible_on_screen, children_of_type, clipped_rect, parents_of_type, screen_rect,
};
use agg_gui_automation::{FrameKind, HeadlessWindow, ProbeWidget, UiDriver};

#[test]
fn rust_only_a_mouse_down_reaches_the_probe_under_it_in_its_local_y_up_space() {
    let mut window = HeadlessWindow::new(200.0, 100.0);
    let target = ProbeWidget::new("target").with_bounds(Rect::new(20.0, 10.0, 50.0, 40.0));
    let log = target.log();
    window.add_child(Box::new(target)).expect("added");

    window.on_mouse_down(30.0, 15.0, MouseButton::Left, 1);
    window.on_mouse_up(30.0, 15.0, MouseButton::Left);

    let log = log.borrow();
    let down = log
        .iter()
        .find_map(|e| match e {
            Event::MouseDown { pos, .. } => Some(*pos),
            _ => None,
        })
        .expect("the probe got the press");
    assert_eq!(down, Point::new(10.0, 5.0));
    assert!(log.iter().any(|e| matches!(e, Event::MouseUp { .. })));
}

#[test]
fn rust_only_frames_advance_the_virtual_clock_and_paint_only_when_asked() {
    let mut window = HeadlessWindow::new(64.0, 32.0);
    window.add_child(Box::new(
        ProbeWidget::new("red")
            .with_bounds(Rect::new(0.0, 0.0, 10.0, 10.0))
            .with_background(Color::rgb(1.0, 0.0, 0.0)),
    ));
    let start = agg_gui::clock::now();
    assert!(window.draw(), "a forced frame paints");
    // Bottom-left pixel of a Y-up 10x10 red square: the last framebuffer row
    // as stored top-down, or the first when stored bottom-up.
    let fb = window.driver().current_screen();
    let pixels = fb.pixels();
    let row = |y: u32| &pixels[(y * fb.width() * 4) as usize..][..4];
    assert!(
        row(0)[0] > 200 || row(fb.height() - 1)[0] > 200,
        "the probe's background was painted"
    );
    // Idle: nothing asked to draw, so reactive frames do not paint.
    let driver = window.driver_mut();
    let painted_before = driver.frames_painted();
    for _ in 0..3 {
        driver.pump(FrameKind::Reactive);
    }
    assert_eq!(driver.frames_painted(), painted_before);
    assert_eq!(agg_gui::clock::since(start), FRAME_INTERVAL * 4);

    // Work queued for the UI thread wakes the next reactive frame.
    agg_gui::ui_thread::run_on_idle_after(Duration::from_millis(15), || {
        agg_gui::animation::request_draw();
    });
    let painted = (0..3)
        .map(|_| driver.pump(FrameKind::Reactive))
        .collect::<Vec<_>>();
    assert!(
        painted.contains(&true),
        "the delayed action's draw request paints"
    );
}

#[test]
fn rust_only_rectangles_clipping_and_visibility_follow_the_tree() {
    let mut window = HeadlessWindow::new(100.0, 100.0);
    let inner = ProbeWidget::new("inner").with_bounds(Rect::new(30.0, 30.0, 40.0, 40.0));
    let outer = ProbeWidget::new("outer")
        .with_bounds(Rect::new(50.0, 10.0, 40.0, 40.0))
        .with_child(Box::new(inner));
    window.add_child(Box::new(outer));
    window.draw();
    let root = window.root();
    let viewport = Size::new(100.0, 100.0);
    let inner = window.handle("inner").expect("inner");

    assert_eq!(
        screen_rect(root, &inner),
        Some(Rect::new(80.0, 40.0, 40.0, 40.0))
    );
    // The outer probe clips its children to its own 40x40, and the window to
    // 100x100: 10x10 of the inner probe shows.
    assert_eq!(
        clipped_rect(root, &inner, viewport),
        Some(Rect::new(80.0, 40.0, 10.0, 10.0))
    );
    assert!(actually_visible_on_screen(root, &inner, viewport));

    let outer = window.handle("outer").expect("outer");
    assert_eq!(parents_of_type::<ProbeWidget>(root, &inner).len(), 2);
    assert_eq!(
        children_of_type::<ProbeWidget>(root, &outer),
        vec![inner.clone()]
    );
    assert_eq!(
        inner.downcast::<ProbeWidget>(root).map(|p| p.background()),
        Some(None)
    );

    // Hidden by an ancestor: still placed, no longer visible.
    let root_mut = window.driver_mut().root_mut();
    outer
        .widget_mut(root_mut)
        .and_then(|w| w.as_any_mut())
        .and_then(|a| a.downcast_mut::<ProbeWidget>())
        .expect("outer probe")
        .set_visible(false);
    let root = window.root();
    assert!(screen_rect(root, &inner).is_some());
    assert!(!actually_visible_on_screen(root, &inner, viewport));
}

#[test]
fn rust_only_a_handle_follows_a_reorder_and_detaches_on_removal() {
    let mut window = HeadlessWindow::new(100.0, 100.0);
    window.add_child(Box::new(ProbeWidget::new("a")));
    window.add_child(Box::new(ProbeWidget::new("b")));
    let mut b = window.handle("b").expect("b");
    assert_eq!(b.path(), &[1]);

    window.driver_mut().root_mut().children_mut().swap(0, 1);
    assert!(b.refresh(window.root()));
    assert_eq!(b.path(), &[0]);
    assert_eq!(b.name(window.root()).as_deref(), Some("b"));

    let removed = window.driver_mut().root_mut().children_mut().remove(0);
    assert!(!b.is_attached(window.root()));
    assert!(tree_query::parents(window.root(), &b).is_empty());
    assert!(!actually_visible_on_screen(
        window.root(),
        &b,
        window.size()
    ));
    drop(removed);
}

#[test]
fn rust_only_focused_path_shows_which_probe_has_focus() {
    let mut window = HeadlessWindow::new(100.0, 100.0);
    window.add_child(Box::new(
        ProbeWidget::new("plain").with_bounds(Rect::new(0.0, 0.0, 10.0, 10.0)),
    ));
    window.add_child(Box::new(
        ProbeWidget::new("field")
            .with_bounds(Rect::new(50.0, 50.0, 20.0, 20.0))
            .with_focusable(true),
    ));
    window.on_mouse_down(55.0, 55.0, MouseButton::Left, 1);
    window.on_mouse_up(55.0, 55.0, MouseButton::Left);
    let field = window.handle("field").expect("field");
    let app = window.driver().app();
    assert_eq!(app.focused_path(), field.resolve(app.root()).as_deref());
}
