//! Unit tests for [`super::ColorPicker`]'s Select / Cancel buttons.
//!
//! The framework can route a click straight to a child `Button` (they are
//! the picker's `children()`), so these deliver the click to the button
//! itself — never through `ColorPicker::on_event` — and check that the
//! commit / restore happens with no further events.

use super::*;
use crate::event::Modifiers;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn left(pos: Point, down: bool) -> Event {
    if down {
        Event::MouseDown {
            pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }
    } else {
        Event::MouseUp {
            pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }
    }
}

/// Open the picker and drag the hue so the working colour differs from the
/// cell's starting value.
fn open_and_change_hue(p: &mut ColorPicker) {
    let _ = p.layout(Size::new(300.0, 400.0));
    let r = p.regions();
    p.on_event(&left(Point::new(r.swatch.x + 5.0, r.swatch.y + 5.0), true));
    p.on_event(&left(Point::new(r.swatch.x + 5.0, r.swatch.y + 5.0), false));
    let _ = p.layout(Size::new(300.0, 400.0));
    let r = p.regions();
    let hue_pt = Point::new(r.hue.x + r.hue.width * 0.5, r.hue.y + r.hue.height * 0.5);
    p.on_event(&left(hue_pt, true));
    p.on_event(&left(hue_pt, false));
    let _ = p.layout(Size::new(300.0, 400.0));
}

/// Click the child button at `idx` directly, as the framework's hit-test
/// routing does, bypassing `ColorPicker::on_event`.
fn click_child(p: &mut ColorPicker, idx: usize) {
    let child = &mut p.children_mut()[idx];
    let b = child.bounds();
    let c = Point::new(b.width * 0.5, b.height * 0.5);
    child.on_event(&left(c, true));
    child.on_event(&left(c, false));
}

#[test]
fn one_click_on_select_commits_without_further_events() {
    let start = Color::rgba(1.0, 0.0, 0.0, 1.0);
    let cell = Rc::new(Cell::new(start));
    let selected = Rc::new(RefCell::new(Vec::<Color>::new()));
    let sel = Rc::clone(&selected);
    let mut p = ColorPicker::new(
        Rc::clone(&cell),
        Arc::new(Font::from_slice(FONT_BYTES).unwrap()),
    )
    .on_select(move |c| sel.borrow_mut().push(c));
    open_and_change_hue(&mut p);
    assert!(p.open);
    let working = p.sync_color_from_hsva();
    assert_ne!(working, start, "hue drag should change the working colour");
    // Reset the cell to prove Select (not the live preview) writes it.
    cell.set(start);

    let idx = p.idx_select;
    click_child(&mut p, idx);

    assert_eq!(cell.get(), working, "Select must commit on its own click");
    assert_eq!(selected.borrow().as_slice(), &[working]);
    // The panel closes on the next layout without any pointer event, and
    // that doesn't fire `on_select` a second time.
    let _ = p.layout(Size::new(300.0, 400.0));
    assert!(!p.open);
    assert_eq!(selected.borrow().len(), 1);
}

#[test]
fn one_click_on_cancel_restores_without_further_events() {
    let start = Color::rgba(1.0, 0.0, 0.0, 1.0);
    let cell = Rc::new(Cell::new(start));
    let selected = Rc::new(Cell::new(0usize));
    let sel = Rc::clone(&selected);
    let mut p = ColorPicker::new(
        Rc::clone(&cell),
        Arc::new(Font::from_slice(FONT_BYTES).unwrap()),
    )
    .on_select(move |_| sel.set(sel.get() + 1));
    open_and_change_hue(&mut p);
    // Simulate a live-preview write that Cancel must undo.
    cell.set(p.sync_color_from_hsva());
    assert_ne!(cell.get(), start);

    let idx = p.idx_cancel;
    click_child(&mut p, idx);

    assert_eq!(cell.get(), start, "Cancel must restore on its own click");
    assert_eq!(selected.get(), 0);
    let _ = p.layout(Size::new(300.0, 400.0));
    assert!(!p.open);
}
