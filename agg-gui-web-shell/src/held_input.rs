//! Input that reaches the shell while the app is busy: held in arrival order
//! and delivered as soon as the app can take it, never dropped.
//!
//! The browser runtime (`web::forward`) hands every DOM input event to the
//! app through a `RefCell`. A DOM event dispatched while that cell is already
//! borrowed (an event the browser fires synchronously from a DOM call the app
//! makes, or one arriving while a host callback holds the app) cannot reach
//! the app at that moment; losing it would lose a press or a release, so the
//! click it belonged to would do nothing. [`forward_or_hold`] keeps such an
//! event, and the next delivery (the next input event, or the frame tick's
//! [`forward_or_hold`] with no event) delivers everything held first, in the
//! order it arrived. Input that arrives before the app is built is still
//! dropped: nothing it aimed at exists yet.
//!
//! Platform-neutral so the rules are unit tested natively.

use std::cell::RefCell;
use std::collections::VecDeque;

/// Input events waiting for the app, oldest first.
#[derive(Debug)]
pub(crate) struct HeldInput<E> {
    events: RefCell<VecDeque<E>>,
}

impl<E> HeldInput<E> {
    pub(crate) const fn new() -> Self {
        Self {
            events: RefCell::new(VecDeque::new()),
        }
    }

    /// How many events are waiting.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.events.borrow().len()
    }

    /// Whether nothing is waiting.
    pub(crate) fn is_empty(&self) -> bool {
        self.events.borrow().is_empty()
    }

    fn push(&self, event: E) {
        self.events.borrow_mut().push_back(event);
    }

    /// The oldest waiting event. The queue is borrowed only for the pop, so
    /// an event held while the one taken is being delivered (a re-entrant
    /// forward) joins the queue behind it.
    fn pop(&self) -> Option<E> {
        self.events.borrow_mut().pop_front()
    }

    fn clear(&self) {
        self.events.borrow_mut().clear();
    }
}

/// What [`forward_or_hold`] did with its event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Forwarded {
    /// The app took it (after everything that was waiting).
    Delivered,
    /// The app was borrowed: the event waits for the next delivery.
    Held,
    /// The app is not built yet: the event (and anything waiting) is gone.
    Dropped,
    /// No event was given and nothing was waiting, or the app was busy.
    Nothing,
}

/// Delivers `event` (when there is one) to the app in `app` through
/// `deliver`, after every event `held` was keeping; holds it instead while
/// the app is borrowed. With `event` `None` this only delivers what is
/// waiting (the frame tick's call).
pub(crate) fn forward_or_hold<A, E>(
    app: &RefCell<Option<A>>,
    held: &HeldInput<E>,
    event: Option<E>,
    mut deliver: impl FnMut(&mut A, E),
) -> Forwarded {
    let Ok(mut borrow) = app.try_borrow_mut() else {
        return match event {
            Some(event) => {
                held.push(event);
                Forwarded::Held
            }
            None => Forwarded::Nothing,
        };
    };
    let Some(app) = borrow.as_mut() else {
        held.clear();
        return match event {
            Some(_) => Forwarded::Dropped,
            None => Forwarded::Nothing,
        };
    };
    let given = event.is_some();
    if let Some(event) = event {
        held.push(event);
    }
    let mut delivered = false;
    while let Some(next) = held.pop() {
        deliver(app, next);
        delivered = true;
    }
    if given || delivered {
        Forwarded::Delivered
    } else {
        Forwarded::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The "app": the events it was given, in order.
    type App = Vec<u32>;

    fn deliver(app: &mut App, event: u32) {
        app.push(event);
    }

    #[test]
    fn an_event_reaches_a_free_app_at_once() {
        let app = RefCell::new(Some(App::new()));
        let held = HeldInput::new();
        assert_eq!(
            forward_or_hold(&app, &held, Some(1), deliver),
            Forwarded::Delivered
        );
        assert_eq!(app.borrow().as_ref().unwrap(), &vec![1]);
    }

    #[test]
    fn events_arriving_while_the_app_is_borrowed_are_delivered_next_in_order() {
        let app = RefCell::new(Some(App::new()));
        let held = HeldInput::new();
        {
            let _busy = app.borrow_mut();
            assert_eq!(
                forward_or_hold(&app, &held, Some(1), deliver),
                Forwarded::Held
            );
            assert_eq!(
                forward_or_hold(&app, &held, Some(2), deliver),
                Forwarded::Held
            );
            assert_eq!(held.len(), 2);
        }
        // The next event goes in behind the two that waited.
        assert_eq!(
            forward_or_hold(&app, &held, Some(3), deliver),
            Forwarded::Delivered
        );
        assert_eq!(app.borrow().as_ref().unwrap(), &vec![1, 2, 3]);
        assert_eq!(held.len(), 0);
    }

    #[test]
    fn the_frame_tick_delivers_what_waited_with_no_new_event() {
        let app = RefCell::new(Some(App::new()));
        let held = HeldInput::new();
        {
            let _busy = app.borrow_mut();
            forward_or_hold(&app, &held, Some(7), deliver);
            // A tick while still busy delivers nothing and keeps the event.
            assert_eq!(
                forward_or_hold(&app, &held, None, deliver),
                Forwarded::Nothing
            );
        }
        assert_eq!(
            forward_or_hold(&app, &held, None, deliver),
            Forwarded::Delivered
        );
        assert_eq!(app.borrow().as_ref().unwrap(), &vec![7]);
        assert_eq!(
            forward_or_hold(&app, &held, None, deliver),
            Forwarded::Nothing
        );
    }

    #[test]
    fn an_event_fired_from_inside_a_delivery_follows_it() {
        let app = RefCell::new(Some(App::new()));
        let held = HeldInput::new();
        // Delivering 1 makes the "browser" fire 2 synchronously, while the app is borrowed.
        let result = forward_or_hold(&app, &held, Some(1), |a: &mut App, e| {
            a.push(e);
            if e == 1 {
                assert_eq!(
                    forward_or_hold(&app, &held, Some(2), deliver),
                    Forwarded::Held
                );
            }
        });
        assert_eq!(result, Forwarded::Delivered);
        assert_eq!(app.borrow().as_ref().unwrap(), &vec![1, 2]);
    }

    #[test]
    fn input_before_the_app_is_built_is_dropped() {
        let app: RefCell<Option<App>> = RefCell::new(None);
        let held = HeldInput::new();
        assert_eq!(
            forward_or_hold(&app, &held, Some(1), deliver),
            Forwarded::Dropped
        );
        assert_eq!(held.len(), 0);
        *app.borrow_mut() = Some(App::new());
        assert_eq!(
            forward_or_hold(&app, &held, None, deliver),
            Forwarded::Nothing
        );
        assert!(app.borrow().as_ref().unwrap().is_empty());
    }
}
