//! agg-sharp `MenuTouchTapTests` (Tests/Agg.Tests/Agg.UI/MenuTouchTapTests.cs):
//! a finger driving a [`MenuBar`] and the popup chain it opens. A finger has
//! no hover, so a submenu opens on the tap itself (`handle_left_down`) and
//! stays up after the finger lifts.
//!
//! C# taps are a touch-typed press and release through a `SystemWindow`. The
//! agg-gui touch shells deliver a tap as a synthesised `MouseMove` at the tap
//! point followed by `MouseDown` + `MouseUp`, each stamped with
//! `touch_state::note_touch_event`; [`Harness::tap`] replays exactly that.
//! C#'s "sub menu still open / same instance" is agg-gui's `open_path` still
//! naming the row, with its panel still in the layout stack.
//!
//! `TappingAnotherTitleWhileAMenuIsOpenSwitchesToIt` is a duplicate of
//! `tests_2.rs::mobile_tap_sequence_keeps_other_top_menu_open` (the same
//! touch tap on Edit while File is open switches `open_index` to 1) with
//! `tests_2.rs::tap_on_other_top_menu_switches_open_popup` pinning that the
//! popup then holds Edit's Copy row.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::event::{Event, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::Widget;

use super::super::geometry::BAR_H;
use super::super::model::MenuItem;
use super::{MenuBar, TopMenu};

const VIEWPORT: Size = Size {
    width: 600.0,
    height: 400.0,
};
/// The File title's centre on the bar.
const FILE: Point = Point { x: 8.0, y: 8.0 };
/// Index of the Recent row in File's popup.
const RECENT: usize = 1;

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn centre(r: Rect) -> Point {
    Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5)
}

/// A bar with a File (New, Recent > Report) and Edit (Copy) menu.
struct Harness {
    bar: MenuBar,
    actions: Rc<RefCell<Vec<String>>>,
}

impl Harness {
    fn show() -> Self {
        crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
        crate::touch_state::clear_last_touch_event_for_testing();
        crate::ux_scale::set_ux_scale(1.0);
        crate::widget::set_current_viewport(VIEWPORT);
        let actions = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&actions);
        let mut bar = MenuBar::new(
            test_font(),
            vec![
                TopMenu::new(
                    "File",
                    vec![
                        MenuItem::action("New", "New").into(),
                        MenuItem::submenu(
                            "Recent",
                            vec![MenuItem::action("Report", "Report").into()],
                        )
                        .into(),
                    ],
                ),
                TopMenu::new("Edit", vec![MenuItem::action("Copy", "Copy").into()]),
            ],
            move |action| sink.borrow_mut().push(action.to_string()),
        );
        bar.layout(Size::new(VIEWPORT.width, BAR_H));
        Self { bar, actions }
    }

    /// Row `row` of open panel `level`, in the bar's coordinates.
    fn row(&self, level: usize, row: usize) -> Point {
        let layouts = self
            .bar
            .popup
            .state
            .layouts(&self.bar.popup.items, VIEWPORT);
        centre(layouts[level].rows[row].rect)
    }

    fn open_panels(&self) -> usize {
        if !self.bar.popup.is_open() {
            return 0;
        }
        self.bar
            .popup
            .state
            .layouts(&self.bar.popup.items, VIEWPORT)
            .len()
    }

    fn press(&mut self, pos: Point) {
        self.bar.on_event(&Event::MouseDown {
            pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
    }

    fn release(&mut self, pos: Point) {
        self.bar.on_event(&Event::MouseUp {
            pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
    }

    /// A finger down and up on one spot, as the touch shells deliver it.
    fn tap(&mut self, pos: Point) {
        crate::touch_state::note_touch_event();
        self.bar.on_event(&Event::MouseMove { pos });
        crate::touch_state::note_touch_event();
        self.press(pos);
        crate::touch_state::note_touch_event();
        self.release(pos);
    }

    fn mouse_move(&mut self, pos: Point) {
        crate::touch_state::clear_last_touch_event_for_testing();
        self.bar.on_event(&Event::MouseMove { pos });
    }

    fn mouse_click(&mut self, pos: Point) {
        crate::touch_state::clear_last_touch_event_for_testing();
        self.press(pos);
        self.release(pos);
    }
}

#[test]
fn tapping_a_sub_menu_row_opens_it_and_it_stays_open_after_the_finger_lifts() {
    let _guard = crate::input_profile::profile_test_lock();
    let mut h = Harness::show();
    h.tap(FILE);
    assert_eq!(h.bar.open_index, Some(0));

    h.tap(h.row(0, RECENT));
    assert_eq!(
        h.bar.popup.state.open_path,
        vec![RECENT],
        "tapping a row with a sub menu opens it"
    );
    assert_eq!(
        h.open_panels(),
        2,
        "the sub menu stays up after the finger lifts, so its rows can be tapped"
    );
    assert_eq!(h.bar.open_index, Some(0));

    h.tap(h.row(1, 0));
    assert_eq!(h.actions.borrow().as_slice(), ["Report"]);
    assert_eq!(h.open_panels(), 0, "choosing a row closes the whole chain");
    assert_eq!(h.bar.open_index, None);
    crate::touch_state::clear_last_touch_event_for_testing();
}

#[test]
fn tapping_the_row_of_an_open_sub_menu_again_keeps_it_open() {
    let _guard = crate::input_profile::profile_test_lock();
    let mut h = Harness::show();
    h.tap(FILE);
    h.tap(h.row(0, RECENT));
    assert_eq!(h.open_panels(), 2);

    // handle_left_down sets open_path to the pressed row: a second tap is
    // not a toggle
    h.tap(h.row(0, RECENT));
    assert_eq!(h.bar.popup.state.open_path, vec![RECENT]);
    assert_eq!(h.open_panels(), 2);

    h.tap(h.row(1, 0));
    assert_eq!(h.actions.borrow().as_slice(), ["Report"]);
    crate::touch_state::clear_last_touch_event_for_testing();
}

#[test]
fn tapping_outside_an_open_sub_menu_closes_the_menus() {
    let _guard = crate::input_profile::profile_test_lock();
    let mut h = Harness::show();
    h.tap(FILE);
    h.tap(h.row(0, RECENT));
    assert_eq!(h.open_panels(), 2);

    // The window's bottom-right corner: off the bar and off every panel.
    let corner = Point::new(VIEWPORT.width - 10.0, BAR_H - VIEWPORT.height + 10.0);
    h.tap(corner);
    assert_eq!(h.open_panels(), 0);
    assert_eq!(h.bar.open_index, None);
    assert!(h.actions.borrow().is_empty());
    crate::touch_state::clear_last_touch_event_for_testing();
}

#[test]
fn a_mouse_still_opens_a_sub_menu_on_hover_and_clicks_its_row() {
    let _guard = crate::input_profile::profile_test_lock();
    let mut h = Harness::show();
    h.mouse_click(FILE);
    h.mouse_move(h.row(0, RECENT));
    assert_eq!(
        h.bar.popup.state.open_path,
        vec![RECENT],
        "a mouse hover still opens a sub menu"
    );

    // a press on a sub menu row sets open_path to it, so the sub menu stays
    h.mouse_click(h.row(0, RECENT));
    assert_eq!(
        h.bar.popup.state.open_path,
        vec![RECENT],
        "clicking the row whose sub menu is open keeps it open"
    );
    assert_eq!(h.open_panels(), 2);

    let report = h.row(1, 0);
    h.mouse_move(report);
    h.mouse_click(report);
    assert_eq!(h.actions.borrow().as_slice(), ["Report"]);
    assert_eq!(h.open_panels(), 0);
}
