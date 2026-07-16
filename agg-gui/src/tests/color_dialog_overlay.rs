//! Regression tests for the floating `color_wheel_picker_dialog` swallowing
//! pointer events over its own bounds.
//!
//! The dialog wraps a `ColorWheelPicker` in a `Window` and is dropped into a
//! `Stack` as an aligned overlay (usually behind a `Rebuilder`, mirroring the
//! RichTextEdit toolbar + Widget Gallery colour rows).  Because the `Rebuilder`
//! and `Stack` place the overlay in a slot whose bounds do NOT contain the
//! window's own `(60, 60)` offset, ordinary `hit_test_subtree` routing let
//! clicks over the painted window fall through to whatever sat underneath — a
//! `ComboBox` in the real bug report.  These tests drive the real `App`
//! pointer path and assert the dialog claims its rect exclusively while open.

use super::*;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect};
use crate::widgets::Rebuilder;
use crate::{ColorWheelPicker, Stack};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// Test-only layer that hosts a single `ComboBox` positioned at an explicit
/// rect (fed through a shared cell).  Stands in for the toolbar / gallery row
/// that sits *under* the floating colour dialog.
struct ComboUnderlay {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    combo_rect: Rc<Cell<Rect>>,
}

impl Widget for ComboUnderlay {
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
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        let r = self.combo_rect.get();
        let child = &mut self.children[0];
        child.layout(Size::new(r.width, r.height));
        child.set_bounds(r);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Build the exact tree shape the demo uses: a Stack with a full-bleed
/// underlay (holding the combo) plus an aligned `Rebuilder` that produces the
/// floating colour dialog.
fn build_app(combo_rect: Rc<Cell<Rect>>, selected: Rc<Cell<usize>>) -> App {
    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());

    let combo = ComboBox::new(vec!["Zero", "One", "Two", "Three"], 0, Arc::clone(&font))
        .with_selected_cell(selected);
    let underlay = ComboUnderlay {
        bounds: Rect::default(),
        children: vec![Box::new(combo)],
        combo_rect,
    };

    let build_font = Arc::clone(&font);
    let dialog = Rebuilder::new(
        || 1_u64,
        move || {
            let picker = ColorWheelPicker::new(Color::rgb(0.2, 0.45, 0.88), Arc::clone(&build_font))
                .with_allow_none(true)
                .with_show_alpha(true)
                .with_font_size(12.0);
            crate::color_wheel_picker_dialog(picker, "Highlight colour")
        },
    );

    let root = Stack::new()
        .with_hit_children_only(false)
        .add(Box::new(underlay))
        .add_aligned(Box::new(dialog));

    App::new(Box::new(root))
}

/// Sum the child-bounds offsets down `Stack -> Rebuilder -> Window` to get the
/// window's origin in root (world, Y-up) space, plus its size.
fn window_world_bounds(app: &App) -> Rect {
    let rebuilder = &app.root().children()[1];
    let window = &rebuilder.children()[0];
    let rb = rebuilder.bounds();
    let wb = window.bounds();
    Rect::new(rb.x + wb.x, rb.y + wb.y, wb.width, wb.height)
}

fn combo_open(app: &App) -> bool {
    let combo = &app.root().children()[0].children()[0];
    combo
        .properties()
        .into_iter()
        .find(|(k, _)| *k == "open")
        .map(|(_, v)| v == "true")
        .unwrap_or(false)
}

fn window_visible(app: &App) -> bool {
    app.root().children()[1].children()[0].is_visible()
}

const VP_W: f64 = 420.0;
const VP_H: f64 = 520.0;

/// Position `combo_rect` so that its 24px-tall button strip (local y 0..24)
/// sits centred on the given world (Y-up) point.
fn place_combo_under(combo_rect: &Rc<Cell<Rect>>, world: Point) {
    combo_rect.set(Rect::new(world.x - 60.0, world.y - 12.0, 120.0, 24.0));
}

/// Clicking the dialog's title-bar close button while it overlaps a ComboBox
/// must close the dialog and NOT leak the press/release to the combo beneath.
#[test]
fn test_color_dialog_close_click_does_not_leak_to_combo() {
    let combo_rect = Rc::new(Cell::new(Rect::new(0.0, 0.0, 120.0, 24.0)));
    let selected = Rc::new(Cell::new(0_usize));
    let mut app = build_app(Rc::clone(&combo_rect), Rc::clone(&selected));
    app.layout(Size::new(VP_W, VP_H));

    // Close button center: window-local (w - 10, h - 14) in Y-up.
    let wb = window_world_bounds(&app);
    let close_world = Point::new(wb.x + wb.width - 10.0, wb.y + wb.height - 14.0);
    place_combo_under(&combo_rect, close_world);
    app.layout(Size::new(VP_W, VP_H));

    assert!(window_visible(&app), "dialog should start open");
    assert!(!combo_open(&app), "combo should start closed");

    let screen_y = VP_H - close_world.y;
    app.on_mouse_down(close_world.x, screen_y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(close_world.x, screen_y, MouseButton::Left, Modifiers::default());

    assert!(
        !combo_open(&app),
        "clicking the dialog's close button must not open the ComboBox underneath"
    );
    assert!(
        !window_visible(&app),
        "clicking the close button must close the dialog"
    );
}

/// Clicking the dialog BODY (over the combo, in the region that sticks out of
/// the Rebuilder's slot) must be swallowed — the combo must stay closed.
#[test]
fn test_color_dialog_body_click_does_not_leak_to_combo() {
    let combo_rect = Rc::new(Cell::new(Rect::new(0.0, 0.0, 120.0, 24.0)));
    let selected = Rc::new(Cell::new(0_usize));
    let mut app = build_app(Rc::clone(&combo_rect), Rc::clone(&selected));
    app.layout(Size::new(VP_W, VP_H));

    // A point near the top-right of the window body, below the title bar.
    // TITLE_H is 28; stay a few px below it and near the right edge so the
    // point lands in the Rebuilder's out-of-slot overhang.
    let wb = window_world_bounds(&app);
    let body_world = Point::new(wb.x + wb.width - 6.0, wb.y + wb.height - 34.0);
    place_combo_under(&combo_rect, body_world);
    app.layout(Size::new(VP_W, VP_H));

    assert!(!combo_open(&app), "combo should start closed");

    let screen_y = VP_H - body_world.y;
    app.on_mouse_down(body_world.x, screen_y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(body_world.x, screen_y, MouseButton::Left, Modifiers::default());

    assert!(
        !combo_open(&app),
        "clicking the dialog body must not open the ComboBox underneath"
    );
    assert!(
        window_visible(&app),
        "clicking the dialog body must not close the dialog"
    );
}
