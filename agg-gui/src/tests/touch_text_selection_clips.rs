//! Touch selection handles and toolbar against the edges of what shows
//! (harness in `touch_text_selection.rs`): a handle whose caret a
//! ScrollView (or two, nested) has scrolled out of view is neither drawn
//! nor hit, so a tap there reaches the widget beneath; a `TextArea` part
//! scrolled inside an outer ScrollView keeps only its visible end's handle;
//! and the toolbar flips below a selection at the top of the screen and
//! stays inside its sides.

use std::rc::Rc;

use super::touch_pan_fixtures::{Counts, Probe};
use super::touch_text_selection::{char_at, font, Phone, FINGER, W};
use crate::geometry::{Point, Rect};
use crate::input_profile::InputProfile;
use crate::widgets::touch_selection::{probe, HandleKind, TouchSelectable};
use crate::{FlexColumn, ScrollView, SizedBox, TextArea, TextField, Widget};

/// The first `T` in the tree under `w`, depth first.
fn find<T: 'static>(w: &mut dyn Widget) -> Option<&mut T> {
    if w.as_any_mut().is_some_and(|a| a.is::<T>()) {
        return w.as_any_mut().and_then(|a| a.downcast_mut::<T>());
    }
    for c in w.children_mut().iter_mut() {
        if let Some(found) = find::<T>(c.as_mut()) {
            return Some(found);
        }
    }
    None
}

fn field() -> TextField {
    TextField::new(font())
        .with_text("hello brave new world")
        .with_font_size(16.0)
}

/// A column: `above` points of space, then `child` `height` tall, then a
/// long tail, so the view can scroll.
fn column(above: f64, child: Box<dyn Widget>, height: f64) -> Box<dyn Widget> {
    Box::new(
        FlexColumn::new()
            .add(Box::new(SizedBox::fixed(360.0, above)))
            .add(Box::new(SizedBox::fixed(360.0, height).with_child(child)))
            .add(Box::new(SizedBox::fixed(360.0, 1000.0))),
    )
}

#[test]
fn a_handle_scrolled_out_of_a_scroll_view_is_hidden_and_a_tap_there_reaches_the_widget_below() {
    // The view ends at window y 300; a button bar sits under it. The field's
    // top 15 points show at the bottom of the view; its text line is below
    // the view's edge.
    let counts = Rc::new(Counts::default());
    let view = ScrollView::new(column(85.0, Box::new(field()), 40.0));
    let rect = Rect::new(20.0, 300.0, 360.0, 100.0);
    let root = super::touch_pan_fixtures::Board::new(W, super::touch_text_selection::H)
        .place(rect, Box::new(view))
        .place(
            Rect::new(20.0, 240.0, 360.0, 60.0),
            Box::new(Probe::new(Rc::clone(&counts))),
        );
    let mut p = Phone::with_root(InputProfile::MobileAndroid, rect, Box::new(root));
    p.long_press(Point::new(rect.x + 90.0, 308.0));
    FINGER.up(&mut p.app);
    p.frame();
    let f = find::<TextField>(p.app.root_mut()).expect("field");
    let (a, c) = f.ts_anchor_cursor();
    assert_ne!(a, c, "the long press selected a word");
    assert!(f.touch_selection_state().selection_handles_shown());
    assert!(
        probe::handle_center(&*f, HandleKind::End).is_none(),
        "its caret is outside the view: no handle"
    );
    // Where the end handle would hang, under the view.
    let presses = counts.presses.get();
    p.tap(Point::new(rect.x + 100.0, 280.0));
    assert_eq!(counts.presses.get(), presses + 1, "the bar got the tap");
}

#[test]
fn handles_inside_nested_scroll_views_follow_the_outer_clip() {
    let inner = ScrollView::new(column(20.0, Box::new(field()), 40.0));
    let outer = ScrollView::new(column(100.0, Box::new(inner), 200.0));
    let rect = Rect::new(20.0, 300.0, 360.0, 300.0);
    let mut p = Phone::new(InputProfile::MobileAndroid, rect, Box::new(outer));
    // The field: 100 + 20 below the outer view's top (600), so 440..480.
    p.long_press(Point::new(rect.x + 90.0, 460.0));
    FINGER.up(&mut p.app);
    p.frame();
    let f = find::<TextField>(p.app.root_mut()).expect("field");
    assert!(
        probe::handle_center(&*f, HandleKind::End).is_some(),
        "in view"
    );
    // Scroll the outer view until the field's line passes its top edge.
    p.widget::<ScrollView>().set_scroll_offset(150.0);
    p.frame();
    let f = find::<TextField>(p.app.root_mut()).expect("field");
    assert!(f.touch_selection_state().selection_handles_shown());
    assert!(
        probe::handle_center(&*f, HandleKind::End).is_none(),
        "scrolled out by the outer view"
    );
    assert!(probe::toolbar_rect(&*f).is_none(), "nor its toolbar");
}

#[test]
fn a_text_area_part_scrolled_out_of_a_scroll_view_keeps_only_its_visible_handle() {
    let text = "alpha beta gamma\ndelta epsilon zeta\neta theta iota\nkappa lambda mu";
    let area = TextArea::new(font()).with_text(text).with_font_size(16.0);
    let view = ScrollView::new(column(150.0, Box::new(area), 200.0));
    let rect = Rect::new(20.0, 300.0, 360.0, 200.0);
    let mut p = Phone::new(InputProfile::MobileAndroid, rect, Box::new(view));
    // The area is at window y 150..350: only its first two lines show.
    let a = find::<TextArea>(p.app.root_mut()).expect("area");
    let first = a.ts_caret_line(7).middle();
    let area_origin_y = 150.0;
    p.long_press(Point::new(rect.x + first.x, area_origin_y + first.y));
    let fourth = {
        let a = find::<TextArea>(p.app.root_mut()).expect("area");
        a.ts_caret_line(text.len() - 1).middle()
    };
    // Slide to the last line, out of the view: "beta ... kappa lambda mu".
    FINGER.move_to(&mut p.app, rect.x + fourth.x, area_origin_y + fourth.y);
    FINGER.up(&mut p.app);
    p.frame();
    let a = find::<TextArea>(p.app.root_mut()).expect("area");
    let (s, e) = a.ts_anchor_cursor();
    assert_eq!((s.min(e), s.max(e)), (6, text.len()));
    assert!(
        probe::handle_center(&*a, HandleKind::Start).is_some(),
        "line 1 shows"
    );
    assert!(
        probe::handle_center(&*a, HandleKind::End).is_none(),
        "line 4 does not"
    );
}

#[test]
fn the_toolbar_flips_below_a_selection_at_the_top_and_stays_inside_the_sides() {
    // A field against the top right corner of the screen.
    let rect = Rect::new(W - 200.0, 800.0 - 40.0, 200.0, 40.0);
    let mut p = Phone::new(InputProfile::MobileAndroid, rect, Box::new(field()));
    let at = char_at::<TextField>(&mut p, 16); // "world", at the right
    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    let f = p.widget::<TextField>();
    let bar = probe::toolbar_rect(&*f).expect("toolbar");
    let line = f.ts_caret_line(16);
    assert!(
        bar.y + bar.height < line.bottom,
        "below the selection: {bar:?}"
    );
    let left = rect.x + bar.x;
    assert!(
        left >= 0.0 && left + bar.width <= W,
        "inside the screen: {left}"
    );
}
