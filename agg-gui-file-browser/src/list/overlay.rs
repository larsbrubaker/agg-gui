//! What a row or tile hands the widget the app lays over its picture (`ItemPictures::tile_overlay`:
//! a favourite marker). The overlay is the row's child, over the picture square, but the row
//! claims the pointer itself and routes to it ([`OverlayRoute`]), because the overlay needs more
//! than the hits on its own shape: it is told the row's hover (a marker that shows while the row
//! is hovered) and every move over the row in its own coordinates, and it is handed a press only
//! where its `hit_test` says the press is its own; a press it takes is the overlay's alone (no
//! selection, no tap, no drag), and the release and a lost capture of that press go to it too.
//! Over its own shape its tooltip is the row's, and the row reports the overlay's properties after
//! its own. A row the drag carries paints faded from its `paint` to its `finish_paint`, so its
//! overlay, painted between them as its child, fades with it. The overlay is told where its
//! square sits in the row or tile (`OverlayFrame`). A row rebuilt under a still pointer tells its new
//! overlay it is hovered.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::{Event, EventResult, MouseButton, Point, Widget};

use crate::state::BrowserState;

/// The routing of one row's pointer events to its overlay (see the module header).
#[derive(Debug, Default)]
pub struct OverlayRoute {
    /// The overlay took the press under way.
    pressed: bool,
    /// The pointer is over the overlay's own shape.
    over: bool,
}

/// `pos` (the row's coordinates) in `overlay`'s.
fn local(overlay: &dyn Widget, pos: Point) -> Point {
    let b = overlay.bounds();
    Point::new(pos.x - b.x, pos.y - b.y)
}

/// `event` moved into `overlay`'s coordinates.
fn moved(overlay: &dyn Widget, event: &Event) -> Event {
    match event {
        Event::MouseMove { pos } => Event::MouseMove {
            pos: local(overlay, *pos),
        },
        Event::MouseDown {
            pos,
            button,
            modifiers,
        } => Event::MouseDown {
            pos: local(overlay, *pos),
            button: *button,
            modifiers: *modifiers,
        },
        Event::MouseUp {
            pos,
            button,
            modifiers,
        } => Event::MouseUp {
            pos: local(overlay, *pos),
            button: *button,
            modifiers: *modifiers,
        },
        other => other.clone(),
    }
}

impl OverlayRoute {
    /// Tells a new overlay the row it lies over is hovered.
    pub fn hovered(overlay: Option<&mut Box<dyn Widget>>) {
        if let Some(overlay) = overlay {
            overlay.on_event(&Event::MouseEnter);
        }
    }

    /// Routes `event` (the row's coordinates) to `overlay`; `Some` when the overlay took it and
    /// the row must leave it alone. A press the overlay takes forgets any tap of the row
    /// (`state`), so it never activates the row.
    pub fn route(
        &mut self,
        overlay: Option<&mut Box<dyn Widget>>,
        event: &Event,
        state: &Rc<RefCell<BrowserState>>,
    ) -> Option<EventResult> {
        let overlay = overlay?;
        match event {
            Event::MouseEnter => {
                overlay.on_event(event);
                None
            }
            Event::MouseLeave => {
                self.over = false;
                overlay.on_event(event);
                None
            }
            Event::MouseMove { pos } => {
                self.over = overlay.hit_test(local(overlay.as_ref(), *pos));
                let result = overlay.on_event(&moved(overlay.as_ref(), event));
                (self.pressed && result.is_consumed()).then_some(result)
            }
            Event::MouseDown { pos, button, .. } => {
                let own =
                    *button == MouseButton::Left && overlay.hit_test(local(overlay.as_ref(), *pos));
                if !own {
                    return None;
                }
                let result = overlay.on_event(&moved(overlay.as_ref(), event));
                if !result.is_consumed() {
                    return None;
                }
                self.pressed = true;
                state.borrow_mut().begin_tap(None);
                crate::trace(|| "tile press taken by its overlay".to_string());
                Some(result)
            }
            Event::MouseUp { .. } | Event::MouseCaptureLost if self.pressed => {
                self.pressed = false;
                overlay.on_event(&moved(overlay.as_ref(), event));
                Some(EventResult::Consumed)
            }
            _ => None,
        }
    }

    /// The overlay's own properties, which the row reports as its own after them, so a test or
    /// script reading the row finds the overlay's state (a favourite marker's "shown") on it.
    pub fn properties(overlay: Option<&dyn Widget>) -> Vec<(&'static str, String)> {
        overlay.map(|o| o.properties()).unwrap_or_default()
    }

    /// The overlay's tooltip while the pointer is over its own shape.
    pub fn tooltip<'a>(&self, overlay: Option<&'a dyn Widget>) -> Option<&'a str> {
        overlay.filter(|_| self.over)?.tooltip_text()
    }
}
