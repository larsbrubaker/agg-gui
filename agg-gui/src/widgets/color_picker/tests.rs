//! Unit tests for [`super::ColorPicker`]'s panel sub-widgets: the Select /
//! Cancel buttons and the "No Color (Pass Through)" checkbox.
//!
//! The framework can route a click straight to a child `Button` (they are
//! the picker's `children()`), so these deliver the click to the button
//! itself — never through `ColorPicker::on_event` — and check that the
//! commit / restore happens with no further events.  The paint tests run
//! the production `paint_subtree` traversal into a framebuffer: the
//! sub-widgets show only while the panel is open.

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

/// Paint `p` through the production traversal into a `w`x`h` buffer.
fn paint_pixels(p: &mut ColorPicker, w: u32, h: u32) -> Vec<u8> {
    let _ = p.layout(Size::new(300.0, 400.0));
    let mut fb = crate::Framebuffer::new(w, h);
    {
        let mut ctx = crate::GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        paint_subtree(p, &mut ctx);
    }
    fb.pixels().to_vec()
}

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).unwrap())
}

#[test]
fn a_closed_inline_picker_does_not_paint_the_no_color_checkbox() {
    let cell = Rc::new(Cell::new(Color::rgba(0.0, 0.0, 1.0, 1.0)));
    let mut plain = ColorPicker::new(Rc::clone(&cell), font());
    let mut with_none = ColorPicker::new(Rc::clone(&cell), font()).with_allow_none(true);
    let (w, h) = (PANEL_W as u32, SWATCH_H as u32);
    assert!(
        paint_pixels(&mut with_none, w, h) == paint_pixels(&mut plain, w, h),
        "the closed swatch must look the same with or without the No Color option"
    );
}

#[test]
fn a_closed_popup_picker_does_not_paint_the_no_color_checkbox() {
    let cell = Rc::new(Cell::new(Color::rgba(0.0, 0.0, 1.0, 1.0)));
    let mut plain = ColorPicker::new(Rc::clone(&cell), font()).with_round_popup_swatch(24.0);
    let mut with_none = ColorPicker::new(Rc::clone(&cell), font())
        .with_round_popup_swatch(24.0)
        .with_allow_none(true);
    assert!(
        paint_pixels(&mut with_none, 24, 24) == paint_pixels(&mut plain, 24, 24),
        "the closed round swatch must look the same with or without the No Color option"
    );
}

#[test]
fn an_open_inline_picker_paints_the_no_color_checkbox() {
    // The checkbox's box fills with the accent when checked, so its region
    // must differ between the two states once the panel is open.
    let paint_open = |no_color: bool| {
        let cell = Rc::new(Cell::new(Color::rgba(0.0, 0.0, 1.0, 1.0)));
        let mut p = ColorPicker::new(cell, font()).with_allow_none(true);
        let _ = p.layout(Size::new(300.0, 400.0));
        let r = p.regions();
        p.on_event(&left(Point::new(r.swatch.x + 5.0, r.swatch.y + 5.0), true));
        p.on_event(&left(Point::new(r.swatch.x + 5.0, r.swatch.y + 5.0), false));
        assert!(p.open);
        p.none_cell.set(no_color);
        let h = (SWATCH_H + panel_body_h(true)).ceil() as u32;
        let pixels = paint_pixels(&mut p, PANEL_W as u32, h);
        let none = p.regions().none.expect("No Color row");
        let mut region = Vec::new();
        for y in none.y as u32..(none.y + none.height) as u32 {
            let row = (y * PANEL_W as u32 * 4) as usize;
            region.extend_from_slice(
                &pixels[row + none.x as usize * 4..row + (none.x + 30.0) as usize * 4],
            );
        }
        region
    };
    assert!(paint_open(false) != paint_open(true));
}
