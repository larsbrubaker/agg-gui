//! The canvas cursor: which [`agg_gui::CursorIcon`] the wasm runtime writes
//! to the canvas' CSS `cursor`, and when.
//!
//! Mirrors `agg-gui-shell`'s `apply_cursor`: after every input event the
//! current icon goes through a [`WindowCursor`] (so the DOM is touched only
//! when the icon changes), and every tick first runs
//! [`App::refresh_cursor`], so a cursor changed by code
//! ([`agg_gui::invalidate_cursor`]) shows while the mouse rests, not only on
//! the next pointer event. Platform-neutral so it is unit tested natively;
//! `web::input` and `web::frame` apply what it returns.

use agg_gui::{current_cursor_icon, App, CursorIcon, WindowCursor};

/// The cursor last written to the canvas, and the decision to write another.
#[derive(Default)]
pub(crate) struct CanvasCursor {
    window: WindowCursor,
}

impl CanvasCursor {
    /// The icon to write after an input event (or a paint, which resolves
    /// hover itself), `None` when the canvas already shows it.
    pub(crate) fn after_input(&mut self) -> Option<CursorIcon> {
        self.window.update(current_cursor_icon())
    }

    /// The icon to write on a tick: re-resolves a cursor code invalidated
    /// while the mouse rests, then decides as
    /// [`after_input`](Self::after_input) does.
    pub(crate) fn after_tick(&mut self, app: &mut App) -> Option<CursorIcon> {
        app.refresh_cursor();
        self.after_input()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use agg_gui::{
        invalidate_cursor, set_cursor_icon, CursorIcon, DrawCtx, Event, EventResult, Rect, Size,
        Widget,
    };

    use super::*;

    /// A full-window widget that shows whatever cursor code stored last.
    struct CodeCursor {
        bounds: Rect,
        icon: Rc<Cell<CursorIcon>>,
    }

    impl Widget for CodeCursor {
        fn type_name(&self) -> &'static str {
            "CodeCursor"
        }
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn set_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn children(&self) -> &[Box<dyn Widget>] {
            &[]
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            unreachable!("CodeCursor has no children")
        }
        fn layout(&mut self, available: Size) -> Size {
            available
        }
        fn paint(&mut self, _: &mut dyn DrawCtx) {}
        fn on_event(&mut self, event: &Event) -> EventResult {
            if let Event::MouseMove { .. } = event {
                set_cursor_icon(self.icon.get());
                return EventResult::Consumed;
            }
            EventResult::Ignored
        }
    }

    #[test]
    fn rust_only_a_cursor_changed_by_code_reaches_the_canvas_on_the_next_tick() {
        let icon = Rc::new(Cell::new(CursorIcon::Text));
        let mut app = App::new(Box::new(CodeCursor {
            bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
            icon: Rc::clone(&icon),
        }));
        app.layout(Size::new(100.0, 100.0));
        let mut canvas = CanvasCursor::default();

        app.on_mouse_move(50.0, 50.0);
        assert_eq!(canvas.after_input(), Some(CursorIcon::Text));
        assert_eq!(canvas.after_tick(&mut app), None, "nothing changed");

        // Code changes the cursor while the mouse rests: no pointer event
        // follows, so the tick alone must carry it to the canvas, once.
        icon.set(CursorIcon::PointingHand);
        invalidate_cursor();
        assert_eq!(canvas.after_tick(&mut app), Some(CursorIcon::PointingHand));
        assert_eq!(canvas.after_tick(&mut app), None);
    }
}
