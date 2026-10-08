//! `MouseInteractionTests.MouseCapturedSpressesLeaveEvents` and
//! `MouseCapturedSpressesLeaveEventsInButtonsSameAsRectangles`, split out of
//! `mouse_interaction_tests.rs` (the class's shared helpers live there).
//!
//! While a widget holds pointer capture it alone hears about the pointer: it
//! leaves and re-enters as the pointer crosses its own area, every move
//! reaches it (in its own coordinates), and no other widget is entered, left
//! or moved over. The button version watches a real [`Button`] through
//! [`agg_gui::observe_events`] (C#'s `buttonA.MouseEnter += ...`), so the
//! button itself is the widget hit, captured and first under the mouse.

use super::*;

use agg_gui::observe_events;

/// C#'s `FirstWidgetUnderMouse` for a handle.
fn first_under_mouse(w: &HeadlessWindow, h: &WidgetHandle) -> bool {
    under_mouse_state(w.driver().app(), h) == UnderMouseState::FirstUnderMouse
}

fn captured(w: &HeadlessWindow, h: &WidgetHandle) -> bool {
    mouse_captured(w.driver().app(), h)
}

#[test]
fn mouse_captured_spresses_leave_events() {
    let mut container = Box::new(
        ProbeWidget::new("container")
            .with_bounds(Rect::new(0.0, 0.0, 200.0, 200.0))
            .capturing_presses(),
    );
    let (region_a, a) = watch(
        ProbeWidget::new("regionA")
            .with_bounds(Rect::new(10.0, 10.0, 180.0, 180.0))
            .capturing_presses(),
    );
    container.add_child(region_a);
    let (mut container, container_handle) = container_window(container);
    let region_a = child(&container, &[0]);

    // make sure we know we are entered and captured on a down event
    container.on_mouse_down(15.0, 15.0, MouseButton::Left, 1);
    assert!(first_under_mouse(&container, &region_a));
    assert!(captured(&container, &region_a));
    assert!(a.enter.get() == 1);
    assert!(a.leave.get() == 0);
    assert!(a.enter_bounds.get() == 1);
    assert!(a.leave_bounds.get() == 0);
    assert!(a.moves.get() == 0);

    // make sure we stay on top when internal moves occur
    a.reset();
    container.on_mouse_move(16.0, 16.0);
    assert!(first_under_mouse(&container, &region_a));
    assert!(captured(&container, &region_a));
    a.assert_counts(0, 0, 0, 0);
    assert!(a.moves.get() == 1);

    // make sure we see leave events when captured
    a.reset();
    container.on_mouse_move(1.0, 1.0);
    assert!(!first_under_mouse(&container, &container_handle));
    assert!(!first_under_mouse(&container, &region_a));
    assert!(captured(&container, &region_a));
    a.assert_counts(1, 0, 1, 0);
    assert!(a.moves.get() == 1);

    // make sure we see enter events when captured
    a.reset();
    container.on_mouse_move(15.0, 15.0);
    assert!(first_under_mouse(&container, &region_a));
    assert!(captured(&container, &region_a));
    a.assert_counts(0, 1, 0, 1);
    assert!(a.moves.get() == 1);

    // and we are not captured after mouseup above region
    a.reset();
    container.on_mouse_up(15.0, 15.0, MouseButton::Left);
    assert!(!captured(&container, &region_a));
    a.assert_counts(0, 0, 0, 0);
    assert!(a.moves.get() == 0);
    assert!(a.ups.get() == 1);

    // make sure we are not captured after mouseup above off region
    a.reset();
    container.on_mouse_down(15.0, 15.0, MouseButton::Left, 1);
    assert!(captured(&container, &region_a));
    a.assert_counts(0, 0, 0, 0);
    assert!(a.moves.get() == 0);
    container.on_mouse_up(1.0, 1.0, MouseButton::Left);
    assert!(!captured(&container, &region_a));
    a.assert_counts(1, 0, 1, 0);
    assert!(a.moves.get() == 0);
    assert!(a.ups.get() == 1);

    // when captured make sure we see move events even when they are not above us.
    let (region_b, b) = watch(
        ProbeWidget::new("regionB")
            .with_bounds(Rect::new(20.0, 20.0, 160.0, 160.0))
            .capturing_presses(),
    );
    container
        .driver_mut()
        .root_mut()
        .children_mut()
        .first_mut()
        .expect("the container")
        .children_mut()
        .push(region_b);
    let region_b = child(&container, &[1]);

    a.reset();
    // when captured regionA make sure regionB can not see move events
    container.on_mouse_down(15.0, 15.0, MouseButton::Left, 1);
    assert!(captured(&container, &region_a));
    a.assert_counts(0, 1, 0, 1);
    assert!(a.moves.get() == 0);
    assert!(!captured(&container, &region_b));
    b.assert_counts(0, 0, 0, 0);
    assert!(b.moves.get() == 0);

    a.reset();
    container.on_mouse_move(25.0, 25.0);
    assert!(captured(&container, &region_a));
    a.assert_counts(0, 0, 0, 0);
    assert!(a.moves.get() == 1);
    assert!(!captured(&container, &region_b));
    b.assert_counts(0, 0, 0, 0);
    assert!(b.moves.get() == 0);
}

#[test]
fn mouse_captured_spresses_leave_events_in_buttons_same_as_rectangles() {
    let container = ProbeWidget::new("container")
        .with_bounds(Rect::new(0.0, 0.0, 200.0, 200.0))
        .capturing_presses();

    // C# `BoundsRelativeToParent = (0, 0, 180, 180)` then
    // `OriginRelativeParent = (10, 10)`.
    let mut button_a = Box::new(Button::new("", ui_font()).with_name("buttonA"));
    button_a.set_bounds(Rect::new(10.0, 10.0, 180.0, 180.0));
    let a_got_enter = Rc::new(Cell::new(false));
    let a_got_leave = Rc::new(Cell::new(false));
    let a_got_move = Rc::new(Cell::new(false));
    let a_move = Rc::new(Cell::new((0.0, 0.0)));
    let (enter, leave, moved, at) = (
        Rc::clone(&a_got_enter),
        Rc::clone(&a_got_leave),
        Rc::clone(&a_got_move),
        Rc::clone(&a_move),
    );
    let _observer = observe_events(WidgetId::of(button_a.as_ref()), move |event| match event {
        Event::MouseOver => enter.set(true),
        Event::MouseOut => leave.set(true),
        Event::MouseMove { pos } => {
            moved.set(true);
            at.set((pos.x, pos.y));
        }
        _ => {}
    });
    let container = container.with_child(button_a);
    let (mut container, container_handle) = container_window(Box::new(container));
    let button_a = child(&container, &[0]);
    let reset = || {
        a_got_enter.set(false);
        a_got_leave.set(false);
        a_got_move.set(false);
    };

    // make sure we know we are entered and captured on a down event
    container.on_mouse_down(15.0, 15.0, MouseButton::Left, 1);
    assert!(first_under_mouse(&container, &button_a));
    assert!(captured(&container, &button_a));
    assert!(a_got_enter.get());
    assert!(!a_got_leave.get());
    assert!(!a_got_move.get());

    // make sure we stay on top when internal moves occur
    reset();
    container.on_mouse_move(16.0, 16.0);
    assert!(first_under_mouse(&container, &button_a));
    assert!(captured(&container, &button_a));
    assert!(!a_got_enter.get());
    assert!(!a_got_leave.get());
    assert!(a_got_move.get());
    reset();
    container.on_mouse_move(20.0, 20.0);
    // lets prove that the move has been transformed into the correct coordinate system
    assert!(a_move.get() == (10.0, 10.0));
    assert!(first_under_mouse(&container, &button_a));
    assert!(captured(&container, &button_a));
    assert!(!a_got_enter.get());
    assert!(!a_got_leave.get());
    assert!(a_got_move.get());

    // make sure we see leave events when captured
    reset();
    container.on_mouse_move(1.0, 1.0);
    assert!(!first_under_mouse(&container, &container_handle));
    assert!(!first_under_mouse(&container, &button_a));
    assert!(captured(&container, &button_a));
    assert!(!a_got_enter.get());
    assert!(a_got_leave.get());
    assert!(a_got_move.get());

    // make sure we see enter events when captured
    reset();
    container.on_mouse_move(15.0, 15.0);
    assert!(first_under_mouse(&container, &button_a));
    assert!(captured(&container, &button_a));
    assert!(a_got_enter.get());
    assert!(!a_got_leave.get());
    assert!(a_got_move.get());
}
