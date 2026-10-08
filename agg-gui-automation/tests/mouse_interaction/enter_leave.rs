//! `MouseInteractionTests.ValidateEnterAndLeaveEvents` and
//! `ValidateEnterAndLeaveEventsWhenNested`, split out of
//! `mouse_interaction_tests.rs` (the class's shared helpers live there).
//!
//! C#'s handlers read `UnderMouseState` off their widgets while they run;
//! here they read [`under_mouse_state_of`] by [`WidgetId`], so each probe is
//! boxed (its identity fixed) before the handlers naming it are set, and
//! nested afterwards. A handler that finds the wrong state panics, as C#'s
//! throws.

use super::*;

/// One counter a handler bumps.
type Count = Rc<Cell<u32>>;

fn bump(count: &Count) {
    count.set(count.get() + 1);
}

fn state(id: WidgetId) -> UnderMouseState {
    under_mouse_state_of(id)
}

/// C#'s `UiThread.InvokePendingActions()`.
fn invoke_pending_actions() {
    agg_gui::ui_thread::invoke_pending_actions();
}

#[test]
fn validate_enter_and_leave_events() {
    let mouse_enter: Count = Rc::default();
    let mouse_leave: Count = Rc::default();
    let mouse_enter_bounds: Count = Rc::default();
    let mouse_leave_bounds: Count = Rc::default();
    let mouse_down: Count = Rc::default();
    let mouse_up: Count = Rc::default();

    let container = ProbeWidget::new("container")
        .with_bounds(Rect::new(0.0, 0.0, 200.0, 200.0))
        .capturing_presses();

    let mut region_a = Box::new(
        ProbeWidget::new("regionA")
            .with_bounds(Rect::new(10.0, 10.0, 180.0, 180.0))
            .capturing_presses(),
    );
    let region_a_id = WidgetId::of(region_a.as_ref());
    let (enter, leave, enter_bounds, leave_bounds, down, up) = (
        Rc::clone(&mouse_enter),
        Rc::clone(&mouse_leave),
        Rc::clone(&mouse_enter_bounds),
        Rc::clone(&mouse_leave_bounds),
        Rc::clone(&mouse_down),
        Rc::clone(&mouse_up),
    );
    region_a.set_event_handler(move |event| {
        match event {
            Event::MouseDown { .. } => bump(&down),
            Event::MouseUp { .. } => bump(&up),
            Event::MouseOver => {
                if state(region_a_id) == UnderMouseState::NotUnderMouse {
                    panic!("It must be under the mouse.");
                }
                bump(&enter);
            }
            Event::MouseOut => {
                if state(region_a_id) == UnderMouseState::FirstUnderMouse {
                    panic!("It must not be under the mouse.");
                }
                bump(&leave);
            }
            Event::MouseEnter => {
                if state(region_a_id) == UnderMouseState::NotUnderMouse {
                    panic!("It must be under the mouse.");
                }
                bump(&enter_bounds);
            }
            Event::MouseLeave => {
                if state(region_a_id) != UnderMouseState::NotUnderMouse {
                    panic!("It must not be under the mouse.");
                }
                bump(&leave_bounds);
            }
            _ => {}
        }
        EventResult::Ignored
    });
    let container = container.with_child(region_a);
    let (mut container, _) = container_window(Box::new(container));
    let region_a = child(&container, &[0]);
    let region_a_state = |w: &HeadlessWindow| under_mouse_state(w.driver().app(), &region_a);
    let reset = || {
        for c in [
            &mouse_down,
            &mouse_up,
            &mouse_leave,
            &mouse_enter,
            &mouse_leave_bounds,
            &mouse_enter_bounds,
        ] {
            c.set(0);
        }
    };

    assert!(mouse_down.get() == 0);
    assert!(mouse_up.get() == 0);
    assert!(mouse_leave.get() == 0);
    assert!(mouse_enter.get() == 0);
    assert!(mouse_leave_bounds.get() == 0);
    assert!(mouse_enter_bounds.get() == 0);

    // put the mouse into the widget but outside regionA
    reset();
    container.on_mouse_move(5.0, 5.0);
    invoke_pending_actions();
    assert!(mouse_down.get() == 0);
    assert!(mouse_up.get() == 0);
    assert!(region_a_state(&container) == UnderMouseState::NotUnderMouse);
    assert!(mouse_leave.get() == 0);
    assert!(mouse_enter.get() == 0);
    assert!(mouse_leave_bounds.get() == 0);
    assert!(mouse_enter_bounds.get() == 0);

    // move it into regionA
    reset();
    container.on_mouse_move(15.0, 15.0);
    invoke_pending_actions();
    assert!(mouse_down.get() == 0);
    assert!(mouse_up.get() == 0);
    assert!(region_a_state(&container) == UnderMouseState::FirstUnderMouse);
    assert!(mouse_leave.get() == 0);
    assert!(mouse_enter.get() == 1);
    assert!(mouse_leave_bounds.get() == 0);
    assert!(mouse_enter_bounds.get() == 1);

    // now move it inside regionA and make sure it does not re-trigger either event
    reset();
    container.on_mouse_move(16.0, 15.0);
    invoke_pending_actions();
    assert!(mouse_down.get() == 0);
    assert!(mouse_up.get() == 0);
    assert!(region_a_state(&container) == UnderMouseState::FirstUnderMouse);
    assert!(mouse_leave.get() == 0);
    assert!(mouse_enter.get() == 0);
    assert!(mouse_leave_bounds.get() == 0);
    assert!(mouse_enter_bounds.get() == 0);

    // now leave and make sure we see the leave
    reset();
    container.on_mouse_move(-5.0, -5.0);
    invoke_pending_actions();
    assert!(mouse_down.get() == 0);
    assert!(mouse_up.get() == 0);
    assert!(mouse_leave.get() == 1);
    assert!(mouse_enter.get() == 0);
    assert!(mouse_leave_bounds.get() == 1);
    assert!(mouse_enter_bounds.get() == 0);

    // move back on
    reset();
    container.on_mouse_move(16.0, 15.0);
    invoke_pending_actions();
    // now leave only the inside widget and make sure we see the leave
    assert!(mouse_down.get() == 0);
    assert!(mouse_up.get() == 0);
    assert!(mouse_enter.get() == 1);
    assert!(mouse_leave.get() == 0);
    assert!(mouse_leave_bounds.get() == 0);
    assert!(mouse_enter_bounds.get() == 1);

    // move off
    reset();
    container.on_mouse_move(5.0, 5.0);
    invoke_pending_actions();
    assert!(mouse_down.get() == 0);
    assert!(mouse_up.get() == 0);
    assert!(mouse_leave.get() == 1);
    assert!(mouse_enter.get() == 0);
    assert!(mouse_leave_bounds.get() == 1);
    assert!(mouse_enter_bounds.get() == 0);

    // click back on
    reset();
    container.on_mouse_down(16.0, 15.0, MouseButton::Left, 1);
    invoke_pending_actions();
    container.on_mouse_up(16.0, 15.0, MouseButton::Left);
    invoke_pending_actions();
    assert!(mouse_down.get() == 1);
    assert!(mouse_up.get() == 1);
    assert!(mouse_enter.get() == 1);
    assert!(mouse_leave.get() == 0);
    assert!(mouse_leave_bounds.get() == 0);
    assert!(mouse_enter_bounds.get() == 1);

    // click off
    reset();
    container.on_mouse_down(5.0, 5.0, MouseButton::Left, 1);
    invoke_pending_actions();
    container.on_mouse_up(5.0, 5.0, MouseButton::Left);
    invoke_pending_actions();
    assert!(mouse_down.get() == 0);
    assert!(mouse_up.get() == 0);
    assert!(mouse_leave.get() == 1);
    assert!(mouse_enter.get() == 0);
    assert!(mouse_leave_bounds.get() == 1);
    assert!(mouse_enter_bounds.get() == 0);
}

/// The eight counters of the nested test, in C#'s order.
#[derive(Default)]
struct Nested {
    enter_a: Count,
    leave_a: Count,
    enter_bounds_a: Count,
    leave_bounds_a: Count,
    enter_b: Count,
    leave_b: Count,
    enter_bounds_b: Count,
    leave_bounds_b: Count,
}

impl Nested {
    fn reset(&self) {
        for c in [
            &self.enter_a,
            &self.enter_bounds_a,
            &self.leave_a,
            &self.leave_bounds_a,
            &self.enter_b,
            &self.enter_bounds_b,
            &self.leave_b,
            &self.leave_bounds_b,
        ] {
            c.set(0);
        }
    }
}

#[test]
fn validate_enter_and_leave_events_when_nested() {
    // ___container__(200, 200)_______________________________________
    // |                                                             |
    // |    __regionB__(match A)________________________________     |
    // |   |                                                    |    |
    // |   |    __regionA__(10, 10)_________________________    |    |
    // |   |    |                                           |   |    |
    // |   |    |                                           |   |    |
    // |   |    |______________________________(190, 190)___|   |    |
    // |   |______________________________________(match A)_|    |
    // |                                                             |
    // |___________________________________________________(200,200)_|
    let mut container = Box::new(
        ProbeWidget::new("container")
            .with_bounds(Rect::new(0.0, 0.0, 200.0, 200.0))
            .capturing_presses(),
    );
    // C# `regionB.SetBoundsToEncloseChildren()`: regionB covers regionA
    // exactly, so regionA sits at regionB's origin.
    let mut region_b = Box::new(
        ProbeWidget::new("regionB")
            .with_bounds(Rect::new(10.0, 10.0, 180.0, 180.0))
            .capturing_presses(),
    );
    let mut region_a = Box::new(
        ProbeWidget::new("regionA")
            .with_bounds(Rect::new(0.0, 0.0, 180.0, 180.0))
            .capturing_presses(),
    );
    let container_id = WidgetId::of(container.as_ref());
    let region_b_id = WidgetId::of(region_b.as_ref());
    let region_a_id = WidgetId::of(region_a.as_ref());
    use UnderMouseState::{FirstUnderMouse, NotUnderMouse, UnderMouseNotFirst};

    let counts = Rc::new(Nested::default());
    let n = Rc::clone(&counts);
    region_a.set_event_handler(move |event| {
        match event {
            Event::MouseOver => {
                if state(region_a_id) != FirstUnderMouse {
                    panic!("It must be the first under the mouse.");
                }
                if state(region_b_id) != UnderMouseNotFirst {
                    panic!("It must be under the mouse not first.");
                }
                if state(container_id) != UnderMouseNotFirst {
                    panic!("It must be under the mouse not first.");
                }
                bump(&n.enter_a);
            }
            Event::MouseOut => {
                if state(region_a_id) != NotUnderMouse {
                    panic!("It must be not under the mouse.");
                }
                if state(region_b_id) != NotUnderMouse {
                    panic!("It must be not under the mouse.");
                }
                bump(&n.leave_a);
            }
            Event::MouseEnter => {
                if state(region_a_id) != FirstUnderMouse {
                    panic!("It must be the first under the mouse.");
                }
                if state(region_b_id) != UnderMouseNotFirst {
                    panic!("It must be under the mouse not first.");
                }
                if state(container_id) != UnderMouseNotFirst {
                    panic!("It must be under the mouse not first.");
                }
                bump(&n.enter_bounds_a);
            }
            Event::MouseLeave => {
                if state(region_a_id) != NotUnderMouse {
                    panic!("It must be not under the mouse.");
                }
                if state(region_b_id) != NotUnderMouse {
                    panic!("It must be not under the mouse.");
                }
                bump(&n.leave_bounds_a);
            }
            _ => {}
        }
        EventResult::Ignored
    });

    let n = Rc::clone(&counts);
    region_b.set_event_handler(move |event| {
        match event {
            Event::MouseOver => {
                if state(region_a_id) != FirstUnderMouse {
                    panic!("It must be the first under the mouse.");
                }
                if state(region_b_id) != UnderMouseNotFirst {
                    panic!("It must be under the mouse not first.");
                }
                if state(container_id) != UnderMouseNotFirst {
                    panic!("It must be under the mouse not first.");
                }
                bump(&n.enter_b);
            }
            Event::MouseOut => {
                if state(region_a_id) != NotUnderMouse {
                    panic!("It must be not under the mouse.");
                }
                if state(region_b_id) != NotUnderMouse {
                    panic!("It must be not under the mouse.");
                }
                if state(container_id) != NotUnderMouse {
                    panic!("It must be not under the mouse.");
                }
                bump(&n.leave_b);
            }
            Event::MouseEnter => {
                if state(region_b_id) != UnderMouseNotFirst {
                    panic!("It must be under the mouse not first.");
                }
                if state(container_id) != UnderMouseNotFirst {
                    panic!("It must be under the mouse not first.");
                }
                bump(&n.enter_bounds_b);
            }
            Event::MouseLeave => {
                if state(region_b_id) != NotUnderMouse {
                    panic!("It must be not under the mouse.");
                }
                bump(&n.leave_bounds_b);
            }
            _ => {}
        }
        EventResult::Ignored
    });
    region_b.add_child(region_a);
    container.add_child(region_b);
    let (mut container, _) = container_window(container);
    let c = &counts;

    assert!(c.leave_a.get() == 0);
    assert!(c.enter_a.get() == 0);
    assert!(c.leave_b.get() == 0);
    assert!(c.enter_b.get() == 0);

    // put the mouse into the widget but outside regionA and region B
    container.on_mouse_move(5.0, 5.0);
    assert!(c.leave_a.get() == 0);
    assert!(c.enter_a.get() == 0);
    assert!(c.leave_bounds_a.get() == 0);
    assert!(c.enter_bounds_a.get() == 0);
    assert!(c.leave_b.get() == 0);
    assert!(c.enter_b.get() == 0);
    assert!(c.leave_bounds_b.get() == 0);
    assert!(c.enter_bounds_b.get() == 0);

    // move it into regionA
    c.reset();
    container.on_mouse_move(15.0, 15.0);
    assert!(c.leave_a.get() == 0);
    assert!(c.enter_a.get() == 1);
    assert!(c.leave_bounds_a.get() == 0);
    assert!(c.enter_bounds_a.get() == 1);
    assert!(c.leave_b.get() == 0);
    assert!(c.enter_b.get() == 0);
    assert!(c.leave_bounds_b.get() == 0);
    assert!(c.enter_bounds_b.get() == 1);

    // now move it inside regionA and make sure it does not re-trigger either event
    c.reset();
    container.on_mouse_move(16.0, 15.0);
    assert!(c.leave_a.get() == 0);
    assert!(c.enter_a.get() == 0);
    assert!(c.leave_bounds_a.get() == 0);
    assert!(c.enter_bounds_a.get() == 0);
    assert!(c.leave_b.get() == 0);
    assert!(c.enter_b.get() == 0);
    assert!(c.leave_bounds_b.get() == 0);
    assert!(c.enter_bounds_b.get() == 0);

    // now leave and make sure we see the leave
    c.reset();
    container.on_mouse_move(-5.0, -5.0);
    assert!(c.leave_a.get() == 1);
    assert!(c.enter_a.get() == 0);
    assert!(c.leave_bounds_a.get() == 1);
    assert!(c.enter_bounds_a.get() == 0);
    assert!(c.leave_b.get() == 0);
    assert!(c.enter_b.get() == 0);
    assert!(c.leave_bounds_b.get() == 1);
    assert!(c.enter_bounds_b.get() == 0);

    // move back on
    c.reset();
    container.on_mouse_move(16.0, 15.0);
    // now leave only the inside widget and make sure we see the leave
    assert!(c.enter_a.get() == 1);
    assert!(c.leave_a.get() == 0);
    assert!(c.leave_bounds_a.get() == 0);
    assert!(c.enter_bounds_a.get() == 1);
    assert!(c.leave_b.get() == 0);
    assert!(c.enter_b.get() == 0);
    assert!(c.leave_bounds_b.get() == 0);
    assert!(c.enter_bounds_b.get() == 1);

    // and a final leave
    c.reset();
    container.on_mouse_move(5.0, 5.0);
    assert!(c.leave_a.get() == 1);
    assert!(c.enter_a.get() == 0);
    assert!(c.leave_bounds_a.get() == 1);
    assert!(c.enter_bounds_a.get() == 0);
    assert!(c.leave_b.get() == 0);
    assert!(c.enter_b.get() == 0);
    assert!(c.leave_bounds_b.get() == 1);
    assert!(c.enter_bounds_b.get() == 0);

    // click back on
    c.reset();
    container.on_mouse_down(16.0, 15.0, MouseButton::Left, 1);
    invoke_pending_actions();
    container.on_mouse_up(16.0, 15.0, MouseButton::Left);
    invoke_pending_actions();
    assert!(c.enter_a.get() == 1);
    assert!(c.leave_a.get() == 0);
    assert!(c.leave_bounds_a.get() == 0);
    assert!(c.enter_bounds_a.get() == 1);
    assert!(c.leave_b.get() == 0);
    assert!(c.enter_b.get() == 0);
    assert!(c.leave_bounds_b.get() == 0);
    assert!(c.enter_bounds_b.get() == 1);

    // click off
    c.reset();
    container.on_mouse_down(5.0, 5.0, MouseButton::Left, 1);
    invoke_pending_actions();
    container.on_mouse_up(5.0, 5.0, MouseButton::Left);
    invoke_pending_actions();
    assert!(c.leave_a.get() == 1);
    assert!(c.enter_a.get() == 0);
    assert!(c.leave_bounds_a.get() == 1);
    assert!(c.enter_bounds_a.get() == 0);
    assert!(c.leave_b.get() == 0);
    assert!(c.enter_b.get() == 0);
    assert!(c.leave_bounds_b.get() == 1);
    assert!(c.enter_bounds_b.get() == 0);
}
