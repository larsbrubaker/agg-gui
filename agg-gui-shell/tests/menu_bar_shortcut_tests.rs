//! Custom shortcuts on native menu items (`MenuItemModel::shortcut`, an
//! agg-gui `MenuShortcut`): the AppKit key equivalent and modifier mask an
//! item is shown with, and which shown item a Command chord fires. Pure model
//! decisions, so they run on every OS against `agg_gui_shell::menu_bar`.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::widgets::menu::{MenuShortcut, ShortcutKey};
use agg_gui_shell::menu_bar::{
    key_equivalent, match_key_equivalent, match_shortcut, modifier_flags, shortcut_key_equivalent,
    KeyEquivalent, MenuBarModel, MenuItemModel, MenuItemRole,
};

use modifier_flags::{COMMAND, CONTROL, OPTION, SHIFT};

fn sc(text: &str) -> MenuShortcut {
    MenuShortcut::parse(text).expect("shortcut")
}

fn eq(key: &str, modifier_flags: u64) -> KeyEquivalent {
    KeyEquivalent {
        key: key.to_string(),
        modifier_flags,
    }
}

#[test]
fn letters_become_lower_case_with_their_modifiers() {
    assert_eq!(shortcut_key_equivalent(&sc("Ctrl+R")), eq("r", COMMAND));
    assert_eq!(
        shortcut_key_equivalent(&sc("Cmd+Shift+Z")),
        eq("z", COMMAND | SHIFT)
    );
    assert_eq!(
        shortcut_key_equivalent(&sc("Alt+Cmd+I")),
        eq("i", COMMAND | OPTION)
    );
    assert_eq!(shortcut_key_equivalent(&sc("Cmd+1")), eq("1", COMMAND));
    assert_eq!(shortcut_key_equivalent(&sc("F")), eq("f", 0));
}

#[test]
fn named_keys_use_appkit_characters() {
    let k = |key: ShortcutKey| {
        shortcut_key_equivalent(&MenuShortcut {
            key,
            command: true,
            shift: false,
            alt: false,
        })
        .key
    };
    assert_eq!(k(ShortcutKey::Backspace), "\u{8}");
    assert_eq!(k(ShortcutKey::Delete), "\u{F728}");
    assert_eq!(k(ShortcutKey::Enter), "\r");
    assert_eq!(k(ShortcutKey::Escape), "\u{1b}");
    assert_eq!(k(ShortcutKey::Tab), "\t");
    assert_eq!(k(ShortcutKey::Space), " ");
    assert_eq!(k(ShortcutKey::ArrowUp), "\u{F700}");
    assert_eq!(k(ShortcutKey::ArrowDown), "\u{F701}");
    assert_eq!(k(ShortcutKey::ArrowLeft), "\u{F702}");
    assert_eq!(k(ShortcutKey::ArrowRight), "\u{F703}");
    assert_eq!(k(ShortcutKey::Insert), "\u{F727}");
    assert_eq!(k(ShortcutKey::Home), "\u{F729}");
    assert_eq!(k(ShortcutKey::End), "\u{F72B}");
    assert_eq!(k(ShortcutKey::PageUp), "\u{F72C}");
    assert_eq!(k(ShortcutKey::PageDown), "\u{F72D}");
}

#[test]
fn an_items_key_equivalent_is_its_shortcut_else_its_roles_chord() {
    let custom = MenuItemModel::new("Rescan").with_shortcut(sc("Cmd+R"));
    assert_eq!(key_equivalent(&custom), Some(eq("r", COMMAND)));

    let role = MenuItemModel {
        role: MenuItemRole::OpenFile,
        ..MenuItemModel::new("Open")
    };
    assert_eq!(key_equivalent(&role), Some(eq("o", COMMAND)));

    // An explicit shortcut overrides the role's.
    let both = role.clone().with_shortcut(sc("Cmd+Shift+O"));
    assert_eq!(key_equivalent(&both), Some(eq("o", COMMAND | SHIFT)));

    assert_eq!(key_equivalent(&MenuItemModel::new("Plain")), None);
    assert_eq!(key_equivalent(&MenuItemModel::separator()), None);
    let sub = MenuItemModel {
        sub_menu_items: Some(Rc::new(Vec::new)),
        ..MenuItemModel::new("Sub").with_shortcut(sc("Cmd+K"))
    };
    assert_eq!(key_equivalent(&sub), None, "a submenu opens, never runs");
}

fn shown() -> Vec<MenuItemModel> {
    vec![
        MenuItemModel::new("Rescan").with_shortcut(sc("Cmd+R")),
        MenuItemModel::new("Redo").with_shortcut(sc("Cmd+Shift+Z")),
        MenuItemModel::new("Trash").with_shortcut(sc("Cmd+Backspace")),
        MenuItemModel::new("Up").with_shortcut(sc("Cmd+Up")),
        MenuItemModel::new("Filter").with_shortcut(sc("F")),
        MenuItemModel::new("Untouched"),
    ]
}

fn fired(chars: &str, flags: u64) -> Option<String> {
    match_shortcut(&shown(), chars, flags).map(|i| i.text)
}

#[test]
fn a_command_chord_fires_the_shown_item_with_that_shortcut() {
    assert_eq!(fired("r", COMMAND).as_deref(), Some("Rescan"));
    // With Shift down AppKit reports the shifted letter.
    assert_eq!(fired("Z", COMMAND | SHIFT).as_deref(), Some("Redo"));
    // The Delete (backspace) key reports DEL.
    assert_eq!(fired("\u{7f}", COMMAND).as_deref(), Some("Trash"));
    assert_eq!(fired("\u{F700}", COMMAND).as_deref(), Some("Up"));
    // Caps lock is not part of a chord.
    assert_eq!(
        fired("R", COMMAND | modifier_flags::CAPS_LOCK).as_deref(),
        Some("Rescan")
    );
}

#[test]
fn modifiers_must_match_exactly() {
    assert_eq!(fired("r", COMMAND | SHIFT), None);
    assert_eq!(fired("r", COMMAND | OPTION), None);
    assert_eq!(fired("r", COMMAND | CONTROL), None);
    assert_eq!(fired("z", COMMAND), None, "Cmd-Z is not Cmd-Shift-Z");
    assert_eq!(fired("x", COMMAND), None);
}

/// A shortcut without Command is shown but never claimed from the keyboard:
/// AppKit would otherwise take a plain key from text fields and the app's own
/// key handling.
#[test]
fn a_shortcut_without_command_is_shown_but_not_claimed() {
    assert_eq!(fired("f", 0), None);
    assert_eq!(fired("r", 0), None);
}

#[test]
fn hidden_or_disabled_shown_items_do_not_fire() {
    let enabled = Rc::new(Cell::new(false));
    let visible = Rc::new(Cell::new(true));
    let (e, v) = (enabled.clone(), visible.clone());
    let items = vec![MenuItemModel {
        is_enabled: Some(Rc::new(move || e.get())),
        is_visible: Some(Rc::new(move || v.get())),
        ..MenuItemModel::new("Rescan").with_shortcut(sc("Cmd+R"))
    }];
    assert!(match_shortcut(&items, "r", COMMAND).is_none());
    enabled.set(true);
    assert!(match_shortcut(&items, "r", COMMAND).is_some());
    visible.set(false);
    assert!(match_shortcut(&items, "r", COMMAND).is_none());
}

/// The role matcher leaves an item with its own shortcut to `match_shortcut`:
/// its role chord is no longer what it shows.
#[test]
fn a_role_item_with_its_own_shortcut_drops_the_role_chord() {
    let bar = MenuBarModel {
        menus: vec![MenuItemModel {
            sub_menu_items: Some(Rc::new(|| {
                vec![MenuItemModel {
                    role: MenuItemRole::OpenFile,
                    ..MenuItemModel::new("Open").with_shortcut(sc("Cmd+Shift+O"))
                }]
            })),
            ..MenuItemModel::new("File")
        }],
    };
    assert!(match_key_equivalent(Some(&bar), "o", COMMAND).is_none());
}
