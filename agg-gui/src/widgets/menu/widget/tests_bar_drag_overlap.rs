//! agg-sharp `MenuBarWidgetTests.DraggingIntoASubMenuThatOverlapsTheRootMenuUsesTheSubMenuRow`
//! (Tests/Agg.Tests/Agg.UI/MenuBarWidgetTests.cs): a press on a bar title,
//! dragged with the button held through Object > Modify > Transform, where
//! Transform had to open leftward over the root menu, and released on one of
//! Transform's rows.  The row under the pointer is the submenu's, not the
//! root row drawn behind it.  The hover half of the case is
//! `tests_hover.rs::moving_left_into_a_sub_menu_that_overlaps_the_root_menu_keeps_the_chain_open`.
//!
//! Events go straight to the bar in its own space, as `tests_bar_widget.rs`
//! drives it, in C#'s 600 x 400 window.
//!
//! CONVERTED: a leftward submenu overlaps its parent's left edge by the 2 px
//! agg-gui's cascade overlaps on the right (`narrow_sub_menu_placement_tests.rs`),
//! so its right edge is checked against row left + 2 where C# allows + 1.

use std::cell::RefCell;
use std::rc::Rc;

use crate::event::{Event, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::widget::Widget;

use super::super::geometry::BAR_H;
use super::super::model::{MenuEntry, MenuItem};
use super::tests_conformance::{reset_env, test_font};
use super::{MenuBar, TopMenu};

/// C#'s `new SystemWindow(600, 400)`.
const VIEWPORT: Size = Size {
    width: 600.0,
    height: 400.0,
};

const TARGET_TEXT: &str = "Rotate the selection about X";

fn item(text: &str) -> MenuEntry {
    MenuItem::action(text, text).into()
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

fn center(r: Rect) -> Point {
    Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5)
}

fn contains(r: Rect, p: Point) -> bool {
    p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
}

/// The open panels, outermost first.
fn panels(bar: &MenuBar) -> Vec<Rect> {
    bar.popup
        .state
        .layouts(&bar.popup.items, VIEWPORT)
        .iter()
        .map(|l| l.rect)
        .collect()
}

/// The laid out rect of the row at `path` (C# `BoundsOfRow`).
fn row_rect(bar: &MenuBar, path: &[usize]) -> Rect {
    let (last, prefix) = path.split_last().expect("a row path");
    bar.popup
        .state
        .layouts(&bar.popup.items, VIEWPORT)
        .iter()
        .find(|l| l.path_prefix == prefix)
        .and_then(|l| l.rows.iter().find(|r| r.item_index == Some(*last)))
        .unwrap_or_else(|| panic!("row {path:?} is laid out"))
        .rect
}

fn drag_to(bar: &mut MenuBar, pos: Point) {
    bar.on_event(&Event::MouseMove { pos });
}

#[test]
fn dragging_into_a_sub_menu_that_overlaps_the_root_menu_uses_the_sub_menu_row() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    crate::widget::set_current_viewport(VIEWPORT);
    let clicks = Rc::new(RefCell::new(Vec::<String>::new()));
    let fired = Rc::clone(&clicks);
    let object = TopMenu::new(
        "Object",
        vec![
            item("Rename"),
            item("Hide"),
            MenuItem::submenu(
                "Modify",
                vec![
                    item("Plain"),
                    MenuItem::submenu(
                        "Transform",
                        vec![
                            item("Translate the selection along X"),
                            item(TARGET_TEXT),
                            item("Scale the selection along X"),
                        ],
                    )
                    .into(),
                ],
            )
            .into(),
            item("Cut"),
            item("Copy"),
            item("Paste"),
        ],
    );
    // C#'s harness: File and Edit, then the extra menu last.
    let mut bar = MenuBar::new(
        test_font(),
        vec![
            TopMenu::new("File", vec![item("New"), item("Open")]),
            TopMenu::new("Edit", vec![item("Copy")]),
            object,
        ],
        move |action| fired.borrow_mut().push(action.to_string()),
    );
    bar.layout(Size::new(VIEWPORT.width, BAR_H));

    let title = center(bar.children()[2].bounds());
    mouse(&mut bar, title, true);
    assert_eq!(bar.open_index, Some(2));
    let root = panels(&bar)[0];

    let modify = center(row_rect(&bar, &[2]));
    drag_to(&mut bar, modify);
    assert_eq!(
        bar.popup.state.open_path,
        vec![2],
        "Modify's sub menu is up"
    );

    let transform = center(row_rect(&bar, &[2, 1]));
    drag_to(&mut bar, transform);
    assert_eq!(
        bar.popup.state.open_path,
        vec![2, 1],
        "Transform's sub menu is up"
    );

    // The overlap is the whole scenario - pin the layout rather than assume it
    let transform_row = row_rect(&bar, &[2, 1]);
    let open = panels(&bar);
    assert_eq!(open.len(), 3);
    let transform_menu = open[2];
    assert!(
        transform_menu.x + transform_menu.width <= transform_row.x + 2.0,
        "the sub menu did not fit to the right, so it opened leftward ({transform_menu:?} vs row {transform_row:?})"
    );
    assert!(
        transform_menu.x < root.x + root.width && root.x < transform_menu.x + transform_menu.width,
        "opening leftward puts it over the root menu ({transform_menu:?} vs root {root:?})"
    );

    let target = center(row_rect(&bar, &[2, 1, 1]));
    assert!(
        contains(root, target),
        "the row dragged onto has to be one lying over the root menu ({target:?} in {root:?})"
    );

    drag_to(&mut bar, target);
    assert_eq!(bar.open_index, Some(2), "the root menu is still up");
    assert_eq!(bar.popup.state.open_path, vec![2, 1]);
    assert_eq!(
        bar.popup.state.hover_path,
        Some(vec![2, 1, 1]),
        "the obscured root row under the sub menu is not what the drag is on"
    );

    mouse(&mut bar, target, false);

    let clicks = clicks.borrow();
    assert_eq!(
        clicks.iter().filter(|c| c.as_str() == TARGET_TEXT).count(),
        1,
        "the release chooses the sub menu row the pointer is over"
    );
    assert_eq!(
        clicks.iter().filter(|c| c.as_str() == "Cut").count(),
        0,
        "the root row hidden behind the sub menu must not be the one activated"
    );
}
