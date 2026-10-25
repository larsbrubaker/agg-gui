//! Touch text selection in a real [`TextArea`] on an Android-profile phone,
//! through `App`'s touch entry points (harness in `touch_text_selection.rs`):
//! a long press selects the word, the handles drag by character across
//! lines and cannot cross, a handle dragged past the bottom edge scrolls the
//! area, and a vertical swipe over an area inside a `ScrollView` still
//! scrolls the view instead of selecting.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::touch_text_selection::{char_at, font, handle, Phone, FINGER};
use crate::geometry::{Point, Rect};
use crate::input_profile::InputProfile;
use crate::widgets::text_field_core::TextEditState;
use crate::widgets::touch_selection::{HandleKind, TouchSelectable};
use crate::{FlexColumn, ScrollView, SizedBox, TextArea};

const LINES: &str = "alpha beta gamma\ndelta epsilon zeta\neta theta iota\nkappa lambda mu";
/// Where the area sits (Y-up window coordinates).
const AREA: Rect = Rect {
    x: 20.0,
    y: 480.0,
    width: 360.0,
    height: 200.0,
};

fn area_phone(text: &str, rect: Rect) -> Phone {
    let area = TextArea::new(font()).with_text(text).with_font_size(16.0);
    Phone::new(InputProfile::MobileAndroid, rect, Box::new(area))
}

fn selection(p: &mut Phone) -> (usize, usize) {
    let (a, c) = p.widget::<TextArea>().ts_anchor_cursor();
    (a.min(c), a.max(c))
}

/// One visual line's height, from the area's own geometry.
fn line_height(p: &mut Phone) -> f64 {
    let line = p.widget::<TextArea>().ts_caret_line(0);
    line.top - line.bottom
}

#[test]
fn a_long_press_selects_a_word_and_the_handles_drag_across_lines() {
    let mut p = area_phone(LINES, AREA);
    let at = char_at::<TextArea>(&mut p, 7); // the "e" of "beta"
    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    assert_eq!(selection(&mut p), (6, 10), "\"beta\"");
    let ts = p.widget::<TextArea>().touch_selection_state();
    assert!(ts.selection_handles_shown() && ts.toolbar_shown());

    // The end handle, one line down at the same x: "beta gamma\ndelta epsi".
    let lh = line_height(&mut p);
    let from = handle::<TextArea>(&mut p, HandleKind::End);
    p.drag(from, Point::new(from.x, from.y - lh));
    assert_eq!(selection(&mut p), (6, 27));
    // The start handle dragged two lines down stops one character before
    // the end.
    let from = handle::<TextArea>(&mut p, HandleKind::Start);
    p.drag(from, Point::new(from.x + 40.0, from.y - lh * 2.0));
    assert_eq!(selection(&mut p), (26, 27));
}

#[test]
fn sliding_after_a_long_press_extends_by_words_onto_the_next_line() {
    let mut p = area_phone(LINES, AREA);
    let at = char_at::<TextArea>(&mut p, 7);
    p.long_press(at);
    let to = char_at::<TextArea>(&mut p, 25); // "epsilon"
    FINGER.move_to(&mut p.app, to.x, to.y);
    FINGER.up(&mut p.app);
    assert_eq!(selection(&mut p), (6, 30), "\"beta gamma\\ndelta epsilon\"");
}

#[test]
fn a_handle_dragged_past_the_bottom_edge_scrolls_the_area() {
    let text: String = (0..30)
        .map(|i| format!("line {i} words here"))
        .collect::<Vec<_>>()
        .join("\n");
    let rect = Rect::new(20.0, 500.0, 360.0, 120.0);
    let mut p = area_phone(&text, rect);
    assert_eq!(p.widget::<TextArea>().scroll_offset(), 0.0);
    let at = char_at::<TextArea>(&mut p, 1); // "line" on the first line
    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    assert_eq!(selection(&mut p), (0, 4));
    let lh = line_height(&mut p);
    let from = handle::<TextArea>(&mut p, HandleKind::End);
    FINGER.down(&mut p.app, from.x, from.y);
    // Each move below the area's bottom edge aims at a line out of view,
    // which scrolls in.
    let below = rect.y - 10.0;
    for i in 0..6 {
        FINGER.move_to(&mut p.app, from.x, below - i as f64 * 2.0);
        p.frame();
    }
    FINGER.up(&mut p.app);
    let offset = p.widget::<TextArea>().scroll_offset();
    assert!(offset > lh, "the area scrolled: {offset}");
    let visible_lines = (rect.height / lh).floor() as usize;
    let (_, end) = selection(&mut p);
    let end_line = p.widget::<TextArea>().text()[..end].matches('\n').count();
    assert!(
        end_line > visible_lines,
        "the selection reaches past what first showed: line {end_line}"
    );
}

#[test]
fn a_vertical_swipe_over_an_area_in_a_scroll_view_still_scrolls_it() {
    let offset = Rc::new(Cell::new(0.0));
    let edit = Rc::new(RefCell::new(TextEditState::default()));
    let area = TextArea::new(font())
        .with_edit_state(Rc::clone(&edit))
        .with_text(LINES)
        .with_font_size(16.0);
    let column = FlexColumn::new()
        .add(Box::new(
            SizedBox::fixed(360.0, 150.0).with_child(Box::new(area)),
        ))
        .add(Box::new(SizedBox::fixed(360.0, 1000.0)));
    let scroll = ScrollView::new(Box::new(column)).with_offset_cell(Rc::clone(&offset));
    let rect = Rect::new(20.0, 300.0, 360.0, 400.0);
    let mut p = Phone::new(InputProfile::MobileAndroid, rect, Box::new(scroll));
    // A word on the area's third line, selected by a long press (so the
    // swipe below is known to start on the area, and a selection's
    // handles do not take a swipe that misses them).
    let word = Point::new(rect.x + 100.0, rect.y + rect.height - 60.0);
    p.long_press(word);
    FINGER.up(&mut p.app);
    p.frame();
    let selected = {
        let st = edit.borrow();
        (st.anchor, st.cursor)
    };
    assert_ne!(selected.0, selected.1, "the long press selected a word");
    // A vertical swipe from the area's first line.
    let start = Point::new(word.x, word.y + 40.0);
    crate::clock::advance(std::time::Duration::from_secs(1));
    FINGER.down(&mut p.app, start.x, start.y);
    FINGER.move_to(&mut p.app, start.x, start.y + 30.0);
    FINGER.move_to(&mut p.app, start.x, start.y + 80.0);
    FINGER.up(&mut p.app);
    p.frame();
    assert!(offset.get() > 50.0, "the view scrolled: {}", offset.get());
    let st = edit.borrow();
    assert_eq!(
        (st.anchor, st.cursor),
        selected,
        "the swipe selected nothing"
    );
}

#[test]
fn the_handles_follow_the_text_when_the_area_scrolls() {
    let text: String = (0..30)
        .map(|i| format!("line {i} words here"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut p = area_phone(&text, Rect::new(20.0, 500.0, 360.0, 120.0));
    let at = char_at::<TextArea>(&mut p, 23); // "line 1"
    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    let before = handle::<TextArea>(&mut p, HandleKind::End);
    let lh = line_height(&mut p);
    p.widget::<TextArea>().set_scroll_offset(lh);
    p.frame();
    let after = handle::<TextArea>(&mut p, HandleKind::End);
    // (The area snaps its scroll offset to whole pixels.)
    assert!(
        (after.y - before.y - lh).abs() <= 0.5,
        "{before:?} -> {after:?}"
    );
    // Scrolled out of view, the line's handles are gone.
    p.widget::<TextArea>().set_scroll_offset(lh * 10.0);
    p.frame();
    assert!(crate::widgets::touch_selection::probe::handle_center(
        p.widget::<TextArea>(),
        HandleKind::End
    )
    .is_none());
}

#[test]
fn a_handle_held_still_past_the_edge_keeps_scrolling_faster_the_further_out() {
    let text: String = (0..60)
        .map(|i| format!("line {i} words here"))
        .collect::<Vec<_>>()
        .join("\n");
    let rect = Rect::new(20.0, 500.0, 360.0, 120.0);
    let mut p = area_phone(&text, rect);
    let at = char_at::<TextArea>(&mut p, 1);
    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    let from = handle::<TextArea>(&mut p, HandleKind::End);
    FINGER.down(&mut p.app, from.x, from.y);
    // Just past the bottom edge (the handle's line 15 points out), then
    // still: frames keep scrolling without any move.
    let near = rect.y - 15.0;
    FINGER.move_to(&mut p.app, from.x, near);
    p.frame();
    let step = |p: &mut Phone| {
        let before = p.widget::<TextArea>().scroll_offset();
        crate::clock::advance(std::time::Duration::from_millis(50));
        p.frame();
        p.widget::<TextArea>().scroll_offset() - before
    };
    step(&mut p);
    let slow: f64 = (0..4).map(|_| step(&mut p)).sum();
    assert!(slow > 0.0, "a still finger past the edge scrolls");
    assert!(p.app.wants_draw() || p.app.next_draw_deadline().is_some());
    FINGER.move_to(&mut p.app, from.x, near - 60.0);
    p.frame();
    step(&mut p);
    let fast: f64 = (0..4).map(|_| step(&mut p)).sum();
    assert!(
        fast > slow * 2.0,
        "further out scrolls faster: {slow} then {fast}"
    );
    FINGER.up(&mut p.app);
    // Released, it stops.
    p.frame();
    assert_eq!(step(&mut p), 0.0);
}
