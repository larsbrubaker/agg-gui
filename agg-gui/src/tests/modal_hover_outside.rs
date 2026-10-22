//! A modal that lets hover outside it pass
//! ([`Widget::modal_hover_passes_outside`]): the widgets beside a
//! light-dismiss panel still show their cursors while it is open, including
//! after the cursor is re-resolved at paint (`App::refresh_cursor`, which the
//! web shell runs after each tick), while presses outside still go to the
//! modal and a drag the modal captured keeps every move. A modal that keeps
//! the default hides what is beneath, as before.

use super::*;
use crate::{
    current_cursor_icon, invalidate_cursor, set_cursor_icon, CursorIcon, DrawCtx, Event,
    EventResult, Rect, Widget,
};
use std::cell::Cell;
use std::rc::Rc;

const ROOT: f64 = 200.0;

/// A leaf at a fixed rectangle (Y-up) with a resize bar over its leftmost 10
/// units; as a modal it counts the presses and moves it receives.
struct BarWidget {
    bounds: Rect,
    modal: bool,
    passes: bool,
    presses: Rc<Cell<u32>>,
    moves: Rc<Cell<u32>>,
}

impl Widget for BarWidget {
    fn type_name(&self) -> &'static str {
        "BarWidget"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, _b: Rect) {}
    fn children(&self) -> &[Box<dyn Widget>] {
        &[]
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        unreachable!("BarWidget has no children")
    }
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn has_active_modal(&self) -> bool {
        self.modal
    }
    fn modal_hover_passes_outside(&self) -> bool {
        self.passes
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.moves.set(self.moves.get() + 1);
                if pos.x >= 0.0 && pos.x < 10.0 {
                    set_cursor_icon(CursorIcon::ResizeHorizontal);
                }
                EventResult::Consumed
            }
            Event::MouseDown { .. } => {
                self.presses.set(self.presses.get() + 1);
                EventResult::Consumed
            }
            Event::MouseUp { .. } => EventResult::Consumed,
            _ => EventResult::Ignored,
        }
    }
}

struct Root {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Root {
    fn type_name(&self) -> &'static str {
        "Root"
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
    fn layout(&mut self, available: Size) -> Size {
        for c in &mut self.children {
            let b = c.bounds();
            c.layout(Size::new(b.width, b.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// The app with a side panel (bar at x 120..130) on the right and a modal
/// panel (bar at x 0..10) on the left, and the modal's and side panel's
/// press and move counters.
struct Scene {
    app: App,
    modal_presses: Rc<Cell<u32>>,
    modal_moves: Rc<Cell<u32>>,
    side_presses: Rc<Cell<u32>>,
}

fn scene(passes: bool) -> Scene {
    crate::cursor::reset_cursor_icon();
    crate::cursor::take_cursor_invalidation();
    let counter = || Rc::new(Cell::new(0));
    let (modal_presses, modal_moves, side_presses) = (counter(), counter(), counter());
    let side = BarWidget {
        bounds: Rect::new(120.0, 0.0, 80.0, ROOT),
        modal: false,
        passes: false,
        presses: Rc::clone(&side_presses),
        moves: counter(),
    };
    let modal = BarWidget {
        bounds: Rect::new(0.0, 0.0, 100.0, ROOT),
        modal: true,
        passes,
        presses: Rc::clone(&modal_presses),
        moves: Rc::clone(&modal_moves),
    };
    let root = Root {
        bounds: Rect::new(0.0, 0.0, ROOT, ROOT),
        children: vec![Box::new(side), Box::new(modal)],
    };
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(ROOT, ROOT));
    Scene {
        app,
        modal_presses,
        modal_moves,
        side_presses,
    }
}

/// The cursor after a move to `x` (mid-height), and again after the cursor
/// is re-resolved as a paint does.
fn cursor_at(app: &mut App, x: f64) -> (CursorIcon, CursorIcon) {
    app.on_mouse_move(x, ROOT / 2.0);
    let on_move = current_cursor_icon();
    invalidate_cursor();
    app.refresh_cursor();
    (on_move, current_cursor_icon())
}

#[test]
fn hover_beside_a_pass_through_modal_shows_the_cursor_of_what_is_under_the_pointer() {
    let mut s = scene(true);
    let resize = (CursorIcon::ResizeHorizontal, CursorIcon::ResizeHorizontal);
    assert_eq!(cursor_at(&mut s.app, 125.0), resize, "the side panel's bar");
    assert_eq!(cursor_at(&mut s.app, 5.0), resize, "the modal's own bar");
    let arrow = (CursorIcon::Default, CursorIcon::Default);
    assert_eq!(cursor_at(&mut s.app, 50.0), arrow, "inside the modal");
    assert_eq!(cursor_at(&mut s.app, 110.0), arrow, "between them");

    // A press outside still goes to the modal, never to the bar under it.
    s.app.on_mouse_move(125.0, ROOT / 2.0);
    s.app
        .on_mouse_down(125.0, ROOT / 2.0, MouseButton::Left, Modifiers::default());
    s.app
        .on_mouse_up(125.0, ROOT / 2.0, MouseButton::Left, Modifiers::default());
    assert_eq!((s.modal_presses.get(), s.side_presses.get()), (1, 0));
}

#[test]
fn a_drag_a_pass_through_modal_captured_keeps_every_move_outside_it() {
    let mut s = scene(true);
    s.app.on_mouse_move(5.0, ROOT / 2.0);
    s.app
        .on_mouse_down(5.0, ROOT / 2.0, MouseButton::Left, Modifiers::default());
    let before = s.modal_moves.get();
    s.app.on_mouse_move(150.0, ROOT / 2.0);
    s.app.on_mouse_move(160.0, ROOT / 2.0);
    assert_eq!(s.modal_moves.get(), before + 2);
}

#[test]
fn a_modal_that_keeps_hover_hides_the_cursors_beneath_it() {
    let mut s = scene(false);
    let arrow = (CursorIcon::Default, CursorIcon::Default);
    assert_eq!(cursor_at(&mut s.app, 125.0), arrow, "the side panel's bar");
    let resize = (CursorIcon::ResizeHorizontal, CursorIcon::ResizeHorizontal);
    assert_eq!(cursor_at(&mut s.app, 5.0), resize, "the modal's own bar");
}
