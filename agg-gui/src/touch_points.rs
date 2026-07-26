//! # Active touch points — per-finger positions for widgets
//!
//! The multi-touch pipeline aggregates fingers into pinch/rotate
//! gestures ([`crate::touch_state`]) and emulates a primary-finger
//! mouse ([`crate::touch_emulation`]). Widgets that act as virtual
//! gamepads — several on-screen buttons held by different fingers at
//! once — need the raw per-finger positions instead. [`App`'s]
//! touch entry points publish the active set here after every touch
//! event; widgets poll [`active`] during paint/update and hit-test
//! the points against their own controls.
//!
//! Positions are in the same app-local coordinate space widgets see
//! mouse events in (Y-up, keyboard-lift applied).
//!
//! [`App`'s]: crate::widget::App

use std::sync::Mutex;

use crate::geometry::Point;

/// One active finger: platform touch id + current position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchPoint {
    pub id: u64,
    pub pos: Point,
}

static POINTS: Mutex<Vec<TouchPoint>> = Mutex::new(Vec::new());

/// App side: replace the published set (called after every touch
/// start/move/end/cancel).
pub fn publish(points: Vec<TouchPoint>) {
    *POINTS.lock().unwrap() = points;
}

/// Widget side: the currently-active fingers. Empty when no touch is
/// down (or the platform has no touchscreen).
pub fn active() -> Vec<TouchPoint> {
    POINTS.lock().unwrap().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_replaces_the_active_set() {
        publish(vec![TouchPoint {
            id: 7,
            pos: Point::new(10.0, 20.0),
        }]);
        assert_eq!(active().len(), 1);
        assert_eq!(active()[0].id, 7);
        publish(Vec::new());
        assert!(active().is_empty());
    }
}
