//! agg-sharp `PopupMenuHoverTests` (Tests/Agg.Tests/Agg.UI): hover opens
//! submenus with no timer, a sibling row closes the submenu it left open, and
//! hover and keyboard share one highlight.  Driven through the real `App`
//! with `on_mouse_move`, on the window harness of `tests_conformance.rs`.
//!
//! C#'s highlight is the focused row; agg-gui's is `state.hover_path`.  C#
//! builds and closes submenus from the idle queue; agg-gui's open chain is
//! `state.open_path`, updated synchronously, so the C# tests that pin the
//! queue's races (`SweepingPastSiblingSubMenuParentsLeavesTheChainOpen`,
//! `EscapeBeforeAQueuedSubMenuIsShownCancelsIt`) pin their user-visible end
//! state here.  The parent-row fill tests read painted pixels.
//!
//! CONVERTED: in `MovingLeftIntoASubMenuThatOverlapsTheRootMenuKeepsTheChainOpen`
//! a leftward submenu overlaps its parent's left edge by the 2 px agg-gui's
//! cascade overlaps on the right (`narrow_sub_menu_placement_tests.rs`), so
//! its right edge is checked against row left + 2 where C# allows + 1, and
//! the anchor sits at x 100 where C#'s is at 250: agg-gui's panels are a
//! fixed 224 px, wider than C#'s text-fitted ones, so this is where Modify
//! still opens right of the root and Transform no longer fits beside it.
//!
//! Fixes pinned here: a submenu with no room on the right opened clamped over
//! its parent instead of to its left; the rows crossed on the way into an open
//! submenu closed it (now the agg-sharp `SubMenuAimWedge`, `state_aim.rs`);
//! hovering a disabled row dropped the highlight; and re-hovering the open
//! parent row closed the deeper levels below it.

use crate::color::Color;
use crate::event::{Key, Modifiers, MouseButton};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect};
use crate::gfx_ctx::GfxCtx;

use super::super::model::{MenuEntry, MenuItem};
use super::super::paint::MenuStyle;
use super::tests_conformance::{center, click, reset_env, test_font, Harness, VIEWPORT};

fn move_to(h: &mut Harness, p: Point) {
    h.app.on_mouse_move(p.x, VIEWPORT.height - p.y);
}

/// Moves to the centre of the row at `path` (C# `MoveTo(CenterOf(..))`).
fn hover(h: &mut Harness, path: &[usize]) {
    let p = center_of(h, path);
    move_to(h, p);
}

/// C# `MoveAlong`: ten steps on the straight line from `from` to `to`.
fn move_along(h: &mut Harness, from: Point, to: Point) {
    for i in 1..=10 {
        let t = i as f64 / 10.0;
        move_to(
            h,
            Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t),
        );
    }
}

/// The window-space rect of the row at `path` (any open level).
fn rect_of(h: &Harness, path: &[usize]) -> Rect {
    let menu = h.menu.borrow();
    let layouts = menu.state.layouts(&menu.items, VIEWPORT);
    let (last, prefix) = path.split_last().expect("a row path");
    layouts
        .iter()
        .find(|l| l.path_prefix == prefix)
        .and_then(|l| l.rows.iter().find(|r| r.item_index == Some(*last)))
        .map(|r| r.rect)
        .expect("row is laid out")
}

fn center_of(h: &Harness, path: &[usize]) -> Point {
    center(rect_of(h, path))
}

fn highlighted(h: &Harness) -> Option<Vec<usize>> {
    h.menu.borrow().state.hover_path.clone()
}

fn open_path(h: &Harness) -> Vec<usize> {
    h.menu.borrow().state.open_path.clone()
}

fn leaf(label: &str) -> MenuEntry {
    MenuItem::action(label, label).into()
}

fn open_more() -> Vec<MenuEntry> {
    vec![
        leaf("Open"),
        MenuItem::submenu("More", vec![leaf("Leaf")]).into(),
    ]
}

#[test]
fn hover_opens_a_sub_menu_and_its_rows_are_hit_testable() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(open_more());
    hover(&mut h, &[1]);
    assert_eq!(
        open_path(&h),
        vec![1],
        "hovering a sub menu row opens it, with no click and no timer"
    );
    let leaf = center_of(&h, &[1, 0]);
    click(&mut h.app, leaf, MouseButton::Left);
    assert_eq!(*h.actions.borrow(), vec!["Leaf".to_string()]);
}

#[test]
fn hovering_a_plain_sibling_closes_the_sub_menu_and_leaves_the_parent_open() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(open_more());
    hover(&mut h, &[1]);
    assert_eq!(open_path(&h), vec![1]);

    hover(&mut h, &[0]);
    assert!(open_path(&h).is_empty(), "the sub menu closed");
    assert!(h.is_open(), "only the sub menu went away");
    assert_eq!(highlighted(&h), Some(vec![0]));
}

#[test]
fn moving_diagonally_into_an_open_sub_menu_does_not_close_it_on_the_rows_crossed_on_the_way() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let leaves = (1..=5).map(|i| leaf(&format!("Leaf {i}"))).collect();
    let mut h = Harness::show(vec![
        MenuItem::submenu("More", leaves).into(),
        leaf("Second"),
        leaf("Third"),
        leaf("Fourth"),
    ]);
    hover(&mut h, &[0]);
    assert_eq!(open_path(&h), vec![0]);

    let from = center_of(&h, &[0]);
    let to = center_of(&h, &[0, 3]);
    move_along(&mut h, from, to);

    assert_eq!(
        open_path(&h),
        vec![0],
        "the pointer was inside the wedge between the opening row and the sub menu the whole way"
    );
    assert_eq!(highlighted(&h), Some(vec![0, 3]));
}

#[test]
fn hovering_a_different_sub_menu_parent_swaps_the_sub_menus() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![
        MenuItem::submenu("More", vec![leaf("Leaf A")]).into(),
        MenuItem::submenu("Other", vec![leaf("Leaf B")]).into(),
    ]);
    hover(&mut h, &[0]);
    assert_eq!(open_path(&h), vec![0]);
    hover(&mut h, &[1]);
    assert_eq!(open_path(&h), vec![1]);
    assert_eq!(h.panels().len(), 2, "swapped, not stacked");
    assert!(h.is_open());
}

#[test]
fn sweeping_past_sibling_sub_menu_parents_leaves_the_chain_open() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let group = |name: &str, leaf_name: &str| -> MenuEntry {
        MenuItem::submenu(name, vec![leaf(leaf_name)]).into()
    };
    let mut h = Harness::show(vec![
        leaf("Open"),
        MenuItem::submenu(
            "More",
            vec![
                leaf("Plain"),
                group("Group A", "A Leaf"),
                group("Group B", "B Leaf"),
                group("Group C", "C Leaf"),
            ],
        )
        .into(),
    ]);
    hover(&mut h, &[1]);
    for row in 1..=3 {
        hover(&mut h, &[1, row]);
    }
    assert!(h.is_open(), "nothing was chosen or dismissed");
    assert_eq!(
        open_path(&h),
        vec![1, 3],
        "the row the sweep stopped on is open"
    );
    assert_eq!(h.panels().len(), 3);
    let c_leaf = center_of(&h, &[1, 3, 0]);
    click(&mut h.app, c_leaf, MouseButton::Left);
    assert_eq!(*h.actions.borrow(), vec!["C Leaf".to_string()]);
}

#[test]
fn re_hovering_the_open_parent_row_leaves_its_sub_menu_up() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(open_more());
    hover(&mut h, &[1]);
    hover(&mut h, &[1, 0]);
    hover(&mut h, &[1]);
    assert_eq!(open_path(&h), vec![1]);
    assert_eq!(h.panels().len(), 2, "the same one sub menu, not a second");
}

#[test]
fn escape_before_a_queued_sub_menu_is_shown_cancels_it() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(open_more());
    hover(&mut h, &[1]);
    assert_eq!(open_path(&h), vec![1]);

    h.app.on_key_down(Key::Escape, Modifiers::default());

    assert!(
        !h.is_open(),
        "Escape dismisses the menu with a sub menu open"
    );
    assert!(open_path(&h).is_empty(), "no part of the chain is left");
    move_to(&mut h, Point::new(300.0, 100.0));
    assert!(h.panels().is_empty() || !h.is_open());
}

#[test]
fn clicking_a_sub_menu_parent_still_opens_it() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(open_more());
    let more = center_of(&h, &[1]);
    click(&mut h.app, more, MouseButton::Left);
    assert_eq!(open_path(&h), vec![1]);
    assert!(h.is_open());
}

#[test]
fn moving_from_the_parent_row_into_the_sub_menu_keeps_it_open() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(open_more());
    hover(&mut h, &[1]);
    hover(&mut h, &[1, 0]);
    assert_eq!(open_path(&h), vec![1]);
    assert!(h.is_open());
    assert_eq!(
        highlighted(&h),
        Some(vec![1, 0]),
        "the highlight follows the mouse into the sub menu"
    );
}

/// Paints the harness menu over black with a flat grey panel and returns the
/// colour at a point of row `path` clear of its text.
fn row_fill(h: &Harness, path: &[usize]) -> [u8; 3] {
    let style = MenuStyle {
        background: Some(Color::from_rgb8(0xf1, 0xf1, 0xf1)),
        border_width: 0.0,
        shadow: false,
        ..MenuStyle::default()
    };
    h.menu.borrow_mut().set_style(style);
    let rect = rect_of(h, path);
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        h.menu
            .borrow_mut()
            .paint(&mut ctx, test_font(), 14.0, VIEWPORT);
    }
    let (x, y) = (
        (rect.x + 8.0) as usize,
        (rect.y + rect.height - 4.0) as usize,
    );
    let i = (y * fb.width() as usize + x) * 4;
    let px = fb.pixels();
    [px[i], px[i + 1], px[i + 2]]
}

const PANEL: [u8; 3] = [0xf1, 0xf1, 0xf1];

#[test]
fn an_open_sub_menu_keeps_its_parent_row_filled() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(open_more());
    assert_eq!(
        row_fill(&h, &[1]),
        PANEL,
        "untouched, the row draws no fill"
    );

    hover(&mut h, &[1]);
    hover(&mut h, &[1, 0]);
    assert_eq!(highlighted(&h), Some(vec![1, 0]));

    assert_ne!(
        row_fill(&h, &[1]),
        PANEL,
        "the row the open sub menu came out of stays filled"
    );
    assert_eq!(
        row_fill(&h, &[0]),
        PANEL,
        "a row with no sub menu open is untouched"
    );
}

#[test]
fn the_parent_row_fill_goes_when_its_sub_menu_closes() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(open_more());
    hover(&mut h, &[1]);
    assert_eq!(open_path(&h), vec![1]);
    hover(&mut h, &[0]);
    assert!(open_path(&h).is_empty());
    assert_eq!(
        row_fill(&h, &[1]),
        PANEL,
        "nothing is drilled into any more, so the parent row is an ordinary row again"
    );
}

#[test]
fn hovering_an_enabled_row_moves_the_keyboard_highlight() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![leaf("One"), leaf("Two"), leaf("Three")]);
    h.app.on_key_down(Key::ArrowDown, Modifiers::default());
    assert_eq!(highlighted(&h), Some(vec![0]));
    hover(&mut h, &[2]);
    assert_eq!(highlighted(&h), Some(vec![2]));
    hover(&mut h, &[1]);
    assert_eq!(highlighted(&h), Some(vec![1]));
}

#[test]
fn hovering_a_disabled_row_does_not_move_the_highlight() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![
        leaf("One"),
        MenuItem::action("Disabled", "disabled").disabled().into(),
    ]);
    hover(&mut h, &[0]);
    assert_eq!(highlighted(&h), Some(vec![0]));
    let disabled = rect_of(&h, &[1]);
    assert!(disabled.height > 0.0);
    move_to(&mut h, center(disabled));
    assert_eq!(
        highlighted(&h),
        Some(vec![0]),
        "a disabled row cannot take the highlight, so the last enabled one keeps it"
    );
    assert!(h.is_open());
}

#[test]
fn moving_left_into_a_sub_menu_that_overlaps_the_root_menu_keeps_the_chain_open() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show_at(
        vec![
            leaf("Rename"),
            leaf("Hide"),
            MenuItem::submenu(
                "Modify",
                vec![
                    leaf("Plain"),
                    MenuItem::submenu(
                        "Transform",
                        vec![
                            leaf("Translate the selection far along the axis"),
                            leaf("Rotate the selection far about the axis"),
                            leaf("Scale the selection far along the axis"),
                        ],
                    )
                    .into(),
                ],
            )
            .into(),
            leaf("Cut"),
            leaf("Copy"),
            leaf("Paste"),
        ],
        Point::new(100.0, 370.0),
    );
    hover(&mut h, &[2]);
    hover(&mut h, &[2, 1]);
    assert_eq!(open_path(&h), vec![2, 1]);

    let panels = h.panels();
    let (root, transform) = (panels[0], panels[2]);
    let row = rect_of(&h, &[2, 1]);
    assert!(
        transform.x + transform.width <= row.x + 2.0,
        "the sub menu did not fit to the right, so it opened leftward ({transform:?} vs row {row:?})"
    );
    let target = center_of(&h, &[2, 1, 1]);
    assert!(
        target.x >= root.x
            && target.x <= root.x + root.width
            && target.y >= root.y
            && target.y <= root.y + root.height,
        "the row moved onto lies over the root menu ({target:?} in {root:?})"
    );

    let from = center_of(&h, &[2, 1]);
    move_along(&mut h, from, target);

    assert!(h.is_open());
    assert_eq!(open_path(&h), vec![2, 1]);
    assert_eq!(
        highlighted(&h),
        Some(vec![2, 1, 1]),
        "the obscured root row under the sub menu is not what the pointer is on"
    );
}
