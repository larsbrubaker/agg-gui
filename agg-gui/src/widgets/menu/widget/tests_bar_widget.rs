//! agg-sharp `MenuBarWidgetTests` (Tests/Agg.Tests/Agg.UI) not already
//! pinned by `tests_1.rs` / `tests_2.rs`: a press in the bar's neutral
//! space, a press on a sibling title with no move before it, and a drag
//! across titles that ends on a row. Events go straight to the bar in its
//! own space, as those files drive it.

use std::cell::RefCell;
use std::rc::Rc;

use crate::event::{Event, Modifiers, MouseButton};
use crate::geometry::{Point, Size};
use crate::widget::Widget;

use super::super::geometry::BAR_H;
use super::super::model::MenuItem;
use super::tests_conformance::{reset_env, test_font};
use super::{MenuBar, TopMenu};

const VIEWPORT: Size = Size {
    width: 300.0,
    height: 180.0,
};

/// C# `MenuBarHarness`: File (New) and Edit (Copy), recording actions.
fn bar() -> (MenuBar, Rc<RefCell<Vec<String>>>) {
    crate::widget::set_current_viewport(VIEWPORT);
    let actions = Rc::new(RefCell::new(Vec::new()));
    let fired = Rc::clone(&actions);
    let mut bar = MenuBar::new(
        test_font(),
        vec![
            TopMenu::new("File", vec![MenuItem::action("New", "New").into()]),
            TopMenu::new("Edit", vec![MenuItem::action("Copy", "Copy").into()]),
        ],
        move |action| fired.borrow_mut().push(action.to_string()),
    );
    bar.layout(Size::new(VIEWPORT.width, BAR_H));
    (bar, actions)
}

fn title_center(bar: &MenuBar, index: usize) -> Point {
    let r = bar.children()[index].bounds();
    Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5)
}

fn mouse(bar: &mut MenuBar, pos: Point, down: bool) {
    let (button, modifiers) = (MouseButton::Left, Modifiers::default());
    bar.on_event(&if down {
        Event::MouseDown {
            pos,
            button,
            modifiers,
        }
    } else {
        Event::MouseUp {
            pos,
            button,
            modifiers,
        }
    });
}

fn click(bar: &mut MenuBar, pos: Point) {
    mouse(bar, pos, true);
    mouse(bar, pos, false);
}

fn open_labels(bar: &MenuBar) -> Vec<String> {
    match bar.popup.is_open() {
        true => bar
            .popup
            .items
            .iter()
            .filter_map(|e| match e {
                super::super::model::MenuEntry::Item(item) => Some(item.label.clone()),
                super::super::model::MenuEntry::Separator => None,
            })
            .collect(),
        false => Vec::new(),
    }
}

#[test]
fn click_in_neutral_space_closes_the_menu() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (mut bar, _) = bar();
    let t = title_center(&bar, 0);
    click(&mut bar, t);
    assert!(bar.popup.is_open());

    let neutral = Point::new(VIEWPORT.width - 20.0, BAR_H * 0.5);
    let edit = bar.children()[1].bounds();
    assert!(
        neutral.x > edit.x + edit.width,
        "the dismissing press has to land off every title"
    );
    click(&mut bar, neutral);

    assert!(!bar.popup.is_open());
    assert_eq!(bar.open_index, None);
}

#[test]
fn pressing_a_sibling_title_switches_without_a_mouse_move() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (mut bar, _) = bar();
    let t = title_center(&bar, 0);
    click(&mut bar, t);
    assert_eq!(bar.open_index, Some(0));

    let t = title_center(&bar, 1);
    click(&mut bar, t);

    assert_eq!(
        bar.open_index,
        Some(1),
        "pressing a different title must open it, not just close the one that was open"
    );
    assert_eq!(open_labels(&bar), vec!["Copy".to_string()]);
}

#[test]
fn dragging_across_titles_while_pressed_switches_the_menu() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (mut bar, actions) = bar();
    let t = title_center(&bar, 0);
    mouse(&mut bar, t, true);
    assert_eq!(bar.open_index, Some(0));

    let edit = title_center(&bar, 1);
    bar.on_event(&Event::MouseMove { pos: edit });

    assert_eq!(
        bar.open_index,
        Some(1),
        "dragging over a sibling title with the button held must switch to that menu"
    );
    assert_eq!(open_labels(&bar), vec!["Copy".to_string()]);

    let rect = bar.popup.state.layouts(&bar.popup.items, VIEWPORT)[0].rows[0].rect;
    let row = Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
    bar.on_event(&Event::MouseMove { pos: row });
    mouse(&mut bar, row, false);

    assert_eq!(
        *actions.borrow(),
        vec!["Copy".to_string()],
        "the switch has to leave the release still able to choose a row"
    );
}
