//! Keyboard submenu navigation in [`PopupMenuState`], pinned to agg-sharp
//! (MatterCAD's spec, which wins over egui here): `Tests/Agg.Tests/Agg.UI/
//! PopupMenuKeyboardTests.cs` and `MenuBarWidgetTests.cs`, over the
//! `PopupMenu.OnKeyDown` / `MoveHighlight` / `OnRowHover` implementation in
//! `Gui/Menu/PopupMenu.cs`. Each assertion names the C# test it mirrors.
//!
//! agg-sharp's highlight is keyboard focus, and showing a submenu focuses
//! the submenu panel itself (`SystemWindowExtension.ShowPopup`), not one of
//! its rows. So once a submenu opens, by Right, Enter / Space or hover, no
//! row is highlighted and the opener keeps its "open" fill (our
//! `open_path`), and the next Up / Down steps from nothing inside the
//! submenu. A child module of `menu/state.rs`, driving real `KeyDown`
//! events through `handle_event`.

use super::*;
use crate::widgets::menu::model::MenuItem;

/// `Open`, then `More` whose submenu starts with a disabled entry and a
/// separator before its first enabled entry (`Leaf`, path `[1, 2]`), and
/// ends with a nested `Deeper` submenu (`[1, 3]`).
fn items() -> Vec<MenuEntry> {
    vec![
        MenuItem::action("Open", "open").into(),
        MenuItem::submenu(
            "More",
            vec![
                MenuItem::action("Gone", "gone").disabled().into(),
                MenuEntry::Separator,
                MenuItem::action("Leaf", "leaf").into(),
                MenuItem::submenu("Deeper", vec![MenuItem::action("Deep", "deep").into()]).into(),
            ],
        )
        .into(),
    ]
}

fn open_state() -> PopupMenuState {
    let mut state = PopupMenuState::default();
    state.open_at(Point::ORIGIN, MenuAnchorKind::Context);
    state
}

/// Press `key` and return the response; every key here must be consumed
/// (`ArrowKeysAreConsumedSoTheyDoNotReachTheWindowBehindTheMenu`).
fn press(state: &mut PopupMenuState, items: &mut [MenuEntry], key: Key) -> MenuResponse {
    let (result, response) = state.handle_event(
        items,
        &Event::KeyDown {
            key: key.clone(),
            modifiers: Modifiers::default(),
        },
        Size::new(400.0, 300.0),
    );
    assert!(result.is_consumed(), "{key:?} must be consumed");
    response
}

/// `RightOpensTheSubMenuOfTheHighlightedRow` and the Right half of
/// `ArrowRightOnASubmenuRowOpensTheSubmenuNotTheNextMenu`.
#[test]
fn right_opens_the_submenu_of_the_highlighted_row() {
    let mut items = items();
    let mut state = open_state();

    // Right on a row without a submenu has nothing to open
    // (`PopupMenu.OnKeyDown`'s `Keys.Right` arm; a menu bar takes it instead).
    press(&mut state, &mut items, Key::ArrowDown);
    assert_eq!(state.hover_path, Some(vec![0]));
    press(&mut state, &mut items, Key::ArrowRight);
    assert_eq!(state.hover_path, Some(vec![0]));
    assert!(state.open_path.is_empty());

    press(&mut state, &mut items, Key::ArrowDown);
    assert_eq!(state.hover_path, Some(vec![1]));
    press(&mut state, &mut items, Key::ArrowRight);
    assert_eq!(state.open_path, vec![1], "Right opens the submenu");
    assert_eq!(
        state.hover_path, None,
        "the shown submenu takes focus as a panel, so no row is highlighted"
    );

    // `DownFromNothingHighlightsTheFirstEnabledRow` and
    // `SteppingSkipsDisabledRowsAndSeparators`, inside the submenu.
    press(&mut state, &mut items, Key::ArrowDown);
    assert_eq!(state.hover_path, Some(vec![1, 2]));
}

/// `UpFromNothingHighlightsTheLastEnabledRow`, inside a submenu Right opened.
#[test]
fn up_after_right_highlights_the_submenus_last_row() {
    let mut items = items();
    let mut state = open_state();
    press(&mut state, &mut items, Key::ArrowDown);
    press(&mut state, &mut items, Key::ArrowDown);
    press(&mut state, &mut items, Key::ArrowRight);

    press(&mut state, &mut items, Key::ArrowUp);
    assert_eq!(state.hover_path, Some(vec![1, 3]));
}

/// `EnterOnASubMenuRowOpensItsSubMenu`: Enter (and Space, which agg-sharp's
/// `ThemedButton` treats the same) opens the submenu without choosing an
/// action or closing the menu.
#[test]
fn enter_and_space_on_a_submenu_row_open_its_submenu() {
    for key in [Key::Enter, Key::Char(' ')] {
        let mut items = items();
        let mut state = open_state();
        press(&mut state, &mut items, Key::ArrowDown);
        press(&mut state, &mut items, Key::ArrowDown);
        assert_eq!(state.hover_path, Some(vec![1]));

        let response = press(&mut state, &mut items, key.clone());
        assert_eq!(response, MenuResponse::None, "{key:?} chooses no action");
        assert!(state.open, "{key:?} leaves the menu open");
        assert_eq!(state.open_path, vec![1], "{key:?} opens the submenu");
        assert_eq!(state.hover_path, None, "{key:?}: no row highlighted");
    }
}

/// `LeftInASubMenuClosesOnlyThatLevelAndHighlightsItsOpener` and
/// `ArrowLeftInsideASubmenuBacksOutOneLevelOnly`.
#[test]
fn left_in_a_submenu_closes_only_that_level_and_highlights_its_opener() {
    let mut items = items();
    let mut state = open_state();
    // Highlight inside `Deeper`, with `More` and `Deeper` both open.
    state.open_path = vec![1, 3];
    state.hover_path = Some(vec![1, 3, 0]);

    press(&mut state, &mut items, Key::ArrowLeft);
    assert_eq!(state.open_path, vec![1], "Left closes the nested submenu");
    assert_eq!(state.hover_path, Some(vec![1, 3]), "back on `Deeper`");

    press(&mut state, &mut items, Key::ArrowLeft);
    assert!(state.open_path.is_empty(), "Left closes the submenu");
    assert_eq!(state.hover_path, Some(vec![1]), "back on `More`");
    assert!(
        state.open,
        "Left backs out one level, it does not dismiss the menu"
    );

    // Straight after Right, with no submenu row highlighted yet.
    press(&mut state, &mut items, Key::ArrowRight);
    press(&mut state, &mut items, Key::ArrowLeft);
    assert!(state.open_path.is_empty());
    assert_eq!(state.hover_path, Some(vec![1]));
}

/// `LeftInATopLevelMenuDoesNothingAndIsStillConsumed`.
#[test]
fn left_in_a_top_level_menu_does_nothing_and_is_still_consumed() {
    let mut items = items();
    let mut state = open_state();
    press(&mut state, &mut items, Key::ArrowDown);

    press(&mut state, &mut items, Key::ArrowLeft);
    assert!(state.open);
    assert_eq!(state.hover_path, Some(vec![0]), "the highlight stays put");
}

/// `PopupMenu.OnRowHover` focuses the hovered row and opens its submenu, and
/// showing that submenu focuses it as a panel, so Down then steps from
/// nothing inside the submenu. No C# test asserts this directly; it is the
/// implementation the keyboard tests above run against.
#[test]
fn down_after_a_hover_opened_submenu_steps_inside_it() {
    // The opener sits at index 2, which is also a navigable index inside
    // its submenu, so the opener's row can't pass for a submenu row.
    let mut items: Vec<MenuEntry> = vec![
        MenuItem::action("A", "a").into(),
        MenuItem::action("B", "b").into(),
        MenuItem::submenu(
            "Sub",
            ["c0", "c1", "c2", "c3"]
                .into_iter()
                .map(|name| MenuItem::action(name, name).into())
                .collect(),
        )
        .into(),
    ];
    let mut state = open_state();
    // What `update_hover` leaves after the pointer enters `Sub`.
    state.open_path = vec![2];
    state.hover_path = Some(vec![2]);

    press(&mut state, &mut items, Key::ArrowDown);
    assert_eq!(state.hover_path, Some(vec![2, 0]), "Down from nothing");

    state.hover_path = Some(vec![2]);
    press(&mut state, &mut items, Key::ArrowUp);
    assert_eq!(state.hover_path, Some(vec![2, 3]), "Up from nothing");
}

/// `EscapeClosesAnActiveMenu` and `EscapeClosesAWholeSubMenuChain`.
#[test]
fn escape_closes_the_whole_submenu_chain() {
    let mut items = items();
    let mut state = open_state();
    state.open_path = vec![1, 3];
    state.hover_path = Some(vec![1, 3, 0]);

    let response = press(&mut state, &mut items, Key::Escape);
    assert_eq!(response, MenuResponse::Closed);
    assert!(!state.open);
    assert!(state.open_path.is_empty());
}
