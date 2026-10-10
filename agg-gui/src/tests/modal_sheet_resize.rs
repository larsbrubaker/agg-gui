//! Resizable [`ModalSheet`]s (`ModalSheet::with_resizable`): edge and corner
//! drags resize the centred panel symmetrically, honour the minimum size and
//! the host bounds, show the resize cursors, and report every size change —
//! the hooks a desktop dialog needs to remember its size (agg-sharp
//! `DialogWindow` / MatterCAD `DialogPage.WindowSizeSettingsKey`). Sheets
//! that do not opt in stay fixed.

use super::*;
use crate::widgets::ModalSheet;
use crate::{current_cursor_icon, CursorIcon, Rect, Stack};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Host is 400×400; screen coords are Y-down, so a panel centred at
/// (200, 200) has the same rect in both conventions.
const HOST: f64 = 400.0;

struct Harness {
    app: App,
    sizes: Rc<RefCell<Vec<Size>>>,
}

fn harness(resizable: bool, size: Size, min: Size) -> Harness {
    let visible = Rc::new(Cell::new(true));
    let sizes = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&sizes);
    let sheet = ModalSheet::new(visible, Box::new(SizedBox::new()))
        .with_panel_size(size)
        .with_min_panel_size(min)
        .with_resizable(resizable)
        .with_on_size_changed(move |s| log.borrow_mut().push(s));
    let stack = Stack::new()
        .add(Box::new(SizedBox::new()))
        .add(Box::new(sheet));
    let mut app = App::new(Box::new(stack));
    app.layout(Size::new(HOST, HOST));
    Harness { app, sizes }
}

fn sheet(app: &App) -> &ModalSheet {
    app.root().children()[1]
        .as_any()
        .and_then(|a| a.downcast_ref::<ModalSheet>())
        .expect("sheet is the stack's second child")
}

fn drag(app: &mut App, from: (f64, f64), to: (f64, f64)) {
    app.on_mouse_move(from.0, from.1);
    app.on_mouse_down(from.0, from.1, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(to.0, to.1);
    app.on_mouse_up(to.0, to.1, MouseButton::Left, Modifiers::default());
    app.layout(Size::new(HOST, HOST));
}

/// The east edge shows the resize cursor and dragging it widens the
/// centred panel by twice the drag (both edges move), reporting the size.
#[test]
fn east_edge_drag_widens_the_centred_panel() {
    let mut h = harness(true, Size::new(200.0, 100.0), Size::new(120.0, 80.0));
    assert_eq!(
        sheet(&h.app).panel_rect(),
        Rect::new(100.0, 150.0, 200.0, 100.0)
    );

    h.app.on_mouse_move(298.0, 200.0);
    assert_eq!(current_cursor_icon(), CursorIcon::ResizeEast);
    h.app.on_mouse_move(200.0, 200.0);
    assert_eq!(current_cursor_icon(), CursorIcon::Default, "panel interior");

    drag(&mut h.app, (298.0, 200.0), (318.0, 200.0));
    assert_eq!(sheet(&h.app).panel_size(), Size::new(240.0, 100.0));
    assert_eq!(
        sheet(&h.app).panel_rect(),
        Rect::new(80.0, 150.0, 240.0, 100.0)
    );
    assert_eq!(
        h.sizes.borrow().last().copied(),
        Some(Size::new(240.0, 100.0))
    );
}

/// A corner drag resizes both axes; the bottom-right corner is south-east.
#[test]
fn corner_drag_resizes_both_axes() {
    let mut h = harness(true, Size::new(200.0, 100.0), Size::ZERO);
    // Bottom-right corner: x = 300, screen y = 250 (panel bottom).
    h.app.on_mouse_move(298.0, 248.0);
    assert_eq!(current_cursor_icon(), CursorIcon::ResizeSouthEast);
    drag(&mut h.app, (298.0, 248.0), (308.0, 268.0));
    assert_eq!(sheet(&h.app).panel_size(), Size::new(220.0, 140.0));
}

/// Shrinking stops at the minimum size.
#[test]
fn drag_stops_at_the_minimum_size() {
    let mut h = harness(true, Size::new(200.0, 100.0), Size::new(120.0, 80.0));
    drag(&mut h.app, (102.0, 200.0), (190.0, 200.0));
    assert_eq!(sheet(&h.app).panel_size(), Size::new(120.0, 100.0));
    drag(&mut h.app, (200.0, 152.0), (200.0, 190.0));
    assert_eq!(sheet(&h.app).panel_size(), Size::new(120.0, 80.0));
}

/// Growing stops at the host bounds minus the sheet's edge margin.
#[test]
fn drag_stops_at_the_host_bounds() {
    let mut h = harness(true, Size::new(200.0, 100.0), Size::ZERO);
    drag(&mut h.app, (298.0, 200.0), (399.0, 200.0));
    assert_eq!(sheet(&h.app).panel_size(), Size::new(HOST - 48.0, 100.0));
}

/// Sheets are fixed unless they opt in: no cursor, no resize, no callback.
#[test]
fn sheets_are_not_resizable_by_default() {
    let visible = Rc::new(Cell::new(true));
    let plain = ModalSheet::new(visible, Box::new(SizedBox::new()));
    assert!(!plain.is_resizable());

    let mut h = harness(false, Size::new(200.0, 100.0), Size::ZERO);
    h.app.on_mouse_move(298.0, 200.0);
    assert_eq!(current_cursor_icon(), CursorIcon::Default);
    drag(&mut h.app, (298.0, 200.0), (318.0, 200.0));
    assert_eq!(sheet(&h.app).panel_size(), Size::new(200.0, 100.0));
    assert!(h.sizes.borrow().is_empty());
}

/// `set_panel_size` replaces the size on a built sheet (a host restoring a
/// remembered size) without reporting it as a user resize.
#[test]
fn set_panel_size_sets_the_initial_size() {
    let mut h = harness(true, Size::new(200.0, 100.0), Size::ZERO);
    let s = h.app.root_mut().children_mut()[1]
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<ModalSheet>())
        .expect("sheet");
    s.set_panel_size(Size::new(300.0, 250.0));
    h.app.layout(Size::new(HOST, HOST));
    assert_eq!(
        sheet(&h.app).panel_rect(),
        Rect::new(50.0, 75.0, 300.0, 250.0)
    );
    assert!(h.sizes.borrow().is_empty());
}
