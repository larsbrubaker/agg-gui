//! The keyboard-resized layout (`widget/keyboard_resize.rs`), driven through
//! real `App::layout` frames: with
//! `on_screen_keyboard::set_resizes_layout(true)`, focusing a text field lays
//! the root out in the room above the keyboard and shows it raised by that
//! room (so its bottom sits on the keyboard's top and its top stays on
//! screen), a field the shorter layout clipped is scrolled into view by its
//! scroll view, a scroll the user makes afterwards is left alone, and hiding
//! the keyboard gives the root the whole viewport back. With the mode off the
//! root keeps the whole viewport while the keyboard is up.

use super::*;
use crate::geometry::Rect;
use crate::input_profile::{set_input_profile, InputProfile};
use crate::text::Font;
use crate::widget::keyboard_scroll;
use crate::widgets::on_screen_keyboard;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

const VP: (f64, f64) = (400.0, 800.0);
/// The header above the scroll view.
const HEADER: f64 = 200.0;
const FIELDS: usize = 40;
const FIELD_ID_BASE: u64 = 9100;

/// The keyboard on, with the profile lock held and every keyboard global
/// restored on drop (a failing assertion included).
struct Fixture {
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Fixture {
    fn new(resizes: bool) -> Self {
        let lock = crate::input_profile::profile_test_lock();
        set_input_profile(InputProfile::Desktop);
        on_screen_keyboard::test_hook::reset();
        on_screen_keyboard::events::clear();
        keyboard_scroll::reset_lift_for_test();
        on_screen_keyboard::set_enabled(true);
        on_screen_keyboard::set_resizes_layout(resizes);
        Self { _lock: lock }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        on_screen_keyboard::test_hook::reset();
        keyboard_scroll::reset_lift_for_test();
        set_input_profile(InputProfile::Desktop);
    }
}

fn size() -> Size {
    Size::new(VP.0, VP.1)
}

/// A fixed header over a scroll view that takes the rest of the height and
/// holds a column of text fields; the scroll view's offset is published to
/// the returned cell.
fn build_app() -> (App, Rc<Cell<f64>>) {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut column = FlexColumn::new();
    for i in 0..FIELDS {
        column = column.add(Box::new(
            TextField::new(Arc::clone(&font))
                .with_font_size(14.0)
                .with_focus_id(FIELD_ID_BASE + i as u64),
        ));
    }
    let offset = Rc::new(Cell::new(0.0));
    let scroll = ScrollView::new(Box::new(column)).with_offset_cell(Rc::clone(&offset));
    let root = FlexColumn::new()
        .add(Box::new(SizedBox::new().with_height(HEADER)))
        .add_flex(Box::new(scroll), 1.0);
    (App::new(Box::new(root)), offset)
}

fn root_rect(app: &App, path: &[usize]) -> Rect {
    keyboard_scroll::focused_widget_screen_bounds(app.root(), path).expect("the path resolves")
}

/// The field with focus id `FIELD_ID_BASE + index`, in root space.
fn field(app: &App, index: usize) -> Rect {
    root_rect(app, &[1, 0, index])
}

fn scroll_view(app: &App) -> Rect {
    root_rect(app, &[1])
}

fn focus(app: &mut App, index: usize) {
    crate::focus::request_focus(FIELD_ID_BASE + index as u64);
    app.layout(size());
    assert_eq!(app.focused_widget_type_name(), Some("TextField"));
    // The layout after focus is the first in the keyboard's room.
    app.layout(size());
    app.layout(size());
}

/// The lowest field fully shown in the scroll view before the keyboard.
fn lowest_shown_field(app: &App) -> usize {
    let view = scroll_view(app);
    (0..FIELDS)
        .rev()
        .find(|&i| field(app, i).y >= view.y - 0.5)
        .expect("a field shows")
}

#[test]
fn a_focused_field_lays_the_root_out_above_the_keyboard_and_shows_the_field() {
    let _fixture = Fixture::new(true);
    let (mut app, _) = build_app();
    app.layout(size());
    assert_eq!(app.root().bounds().height, VP.1, "the whole viewport");
    let index = lowest_shown_field(&app);
    focus(&mut app, index);
    let panel = on_screen_keyboard::target_panel_height(VP.0);
    assert!(panel > 0.0, "the keyboard has a panel");
    // The root takes the room above the keyboard and is shown raised by it.
    let root = app.root().bounds();
    assert!(
        (root.height - (VP.1 - panel)).abs() < 0.5,
        "{root:?} vs {panel}"
    );
    assert!((keyboard_scroll::lift_target_for_test() - panel).abs() < 0.5);
    // The scroll view shrank under the field, and scrolled it back into view:
    // it lies inside the scroll view, which lies inside the root, above the
    // keyboard on screen.
    let view = scroll_view(&app);
    let f = field(&app, index);
    assert!(view.height < VP.1 - HEADER - panel + 0.5, "{view:?}");
    assert!(
        f.y >= view.y - 0.5 && f.y + f.height <= view.y + view.height + 0.5,
        "the field {f:?} shows in the scroll view {view:?}"
    );
    assert!(f.y >= -0.5, "the field is above the keyboard: {f:?}");
}

#[test]
fn a_scroll_after_the_reveal_stays_and_hiding_the_keyboard_restores_the_layout() {
    let _fixture = Fixture::new(true);
    let (mut app, offset) = build_app();
    app.layout(size());
    let index = lowest_shown_field(&app);
    focus(&mut app, index);
    let revealed = offset.get();
    assert!(revealed > 0.5, "the reveal scrolled: {revealed}");
    // The user scrolls back to the top while typing: later layouts leave it.
    let scroll = app.root_mut().children_mut()[1]
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<ScrollView>())
        .expect("the scroll view");
    scroll.set_scroll_offset(0.0);
    for _ in 0..3 {
        app.layout(size());
    }
    assert_eq!(offset.get(), 0.0, "the user's scroll stays");
    // The keyboard's hide key: focus goes, and the root gets the whole viewport.
    on_screen_keyboard::dismiss();
    app.drain_keyboard_events_for_test();
    app.layout(size());
    assert!(!app.focused_is_text_input());
    assert_eq!(app.root().bounds().height, VP.1);
    assert_eq!(keyboard_scroll::lift_target_for_test(), 0.0);
}

#[test]
fn without_the_mode_the_root_keeps_the_whole_viewport_under_the_keyboard() {
    let _fixture = Fixture::new(false);
    let (mut app, _) = build_app();
    app.layout(size());
    let index = lowest_shown_field(&app);
    focus(&mut app, index);
    assert!(on_screen_keyboard::target_panel_height(VP.0) > 0.0);
    assert_eq!(app.root().bounds().height, VP.1);
}
