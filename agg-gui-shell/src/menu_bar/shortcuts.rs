//! Shortcuts on native menu items: the AppKit key equivalent an item is shown
//! with ([`key_equivalent`]) — its own [`MenuShortcut`] (the in-window
//! `MenuBar`'s shortcut type) or, failing that, its role's standard Command
//! chord — and which shown item a key chord fires ([`match_shortcut`]).
//!
//! Pure decisions, compiled on every OS; `macos.rs` applies them to
//! `NSMenuItem`s and asks [`match_shortcut`] from its
//! `menuHasKeyEquivalent:` delegate method with the items it built.
//!
//! Only chords that include Command are ever claimed from the keyboard. A
//! shortcut without Command (a plain `F`, `Delete`) is still shown in the
//! menu, but AppKit offers key equivalents before the window sees the key, so
//! claiming it would take that key from text fields and from the app's own
//! key handling — which is where such a shortcut runs.  `MenuShortcut` has no
//! separate Control modifier (`command` is Cmd on the mac), so no custom
//! chord uses Control.

use agg_gui::widgets::menu::{MenuShortcut, ShortcutKey};

use super::{is_enabled, key_equivalent_for, modifier_flags, MenuItemModel};

/// What an `NSMenuItem` is given: its `keyEquivalent` string and its
/// `keyEquivalentModifierMask` ([`modifier_flags`] bits).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEquivalent {
    pub key: String,
    pub modifier_flags: u64,
}

/// AppKit's function-key characters (`NSUpArrowFunctionKey` …).
mod function_key {
    pub const UP: char = '\u{F700}';
    pub const DOWN: char = '\u{F701}';
    pub const LEFT: char = '\u{F702}';
    pub const RIGHT: char = '\u{F703}';
    pub const INSERT: char = '\u{F727}';
    pub const DELETE: char = '\u{F728}';
    pub const HOME: char = '\u{F729}';
    pub const END: char = '\u{F72B}';
    pub const PAGE_UP: char = '\u{F72C}';
    pub const PAGE_DOWN: char = '\u{F72D}';
}

/// `NSBackspaceCharacter`: the key equivalent AppKit draws as ⌫.
const BACKSPACE: char = '\u{8}';
/// What the Delete (backspace) key reports as its characters.
const DEL: char = '\u{7f}';
/// What the keypad Enter key reports.
const KEYPAD_ENTER: char = '\u{3}';
/// What Shift-Tab reports (`NSBackTabCharacter`).
const BACK_TAB: char = '\u{19}';

/// The key equivalent `shortcut` is shown with: letters lower case (Shift is
/// in the mask, as AppKit wants), named keys as AppKit's characters.
pub fn shortcut_key_equivalent(shortcut: &MenuShortcut) -> KeyEquivalent {
    use function_key as fk;
    let key = match shortcut.key {
        ShortcutKey::Char(c) => c.to_lowercase().to_string(),
        ShortcutKey::Space => " ".to_string(),
        ShortcutKey::Enter => "\r".to_string(),
        ShortcutKey::Escape => "\u{1b}".to_string(),
        ShortcutKey::Tab => "\t".to_string(),
        ShortcutKey::Backspace => BACKSPACE.to_string(),
        ShortcutKey::Delete => fk::DELETE.to_string(),
        ShortcutKey::Insert => fk::INSERT.to_string(),
        ShortcutKey::ArrowUp => fk::UP.to_string(),
        ShortcutKey::ArrowDown => fk::DOWN.to_string(),
        ShortcutKey::ArrowLeft => fk::LEFT.to_string(),
        ShortcutKey::ArrowRight => fk::RIGHT.to_string(),
        ShortcutKey::Home => fk::HOME.to_string(),
        ShortcutKey::End => fk::END.to_string(),
        ShortcutKey::PageUp => fk::PAGE_UP.to_string(),
        ShortcutKey::PageDown => fk::PAGE_DOWN.to_string(),
    };
    let mut flags = 0;
    if shortcut.command {
        flags |= modifier_flags::COMMAND;
    }
    if shortcut.shift {
        flags |= modifier_flags::SHIFT;
    }
    if shortcut.alt {
        flags |= modifier_flags::OPTION;
    }
    KeyEquivalent {
        key,
        modifier_flags: flags,
    }
}

/// The key equivalent `item` is shown with: its own shortcut, else its
/// role's Command chord ([`key_equivalent_for`]), else none.  Separators and
/// submenus have none.
pub fn key_equivalent(item: &MenuItemModel) -> Option<KeyEquivalent> {
    if item.is_separator || item.sub_menu_items.is_some() {
        return None;
    }
    if let Some(shortcut) = &item.shortcut {
        return Some(shortcut_key_equivalent(shortcut));
    }
    let chord = key_equivalent_for(item.role);
    (!chord.is_empty()).then(|| KeyEquivalent {
        key: chord.to_string(),
        modifier_flags: modifier_flags::COMMAND,
    })
}

/// Whether a key event's `characters` (`charactersIgnoringModifiers`, which
/// keeps Shift) is the key `key` names.
fn same_key(key: &str, characters: &str) -> bool {
    if key.eq_ignore_ascii_case(characters) {
        return true;
    }
    let mut k = key.chars();
    let mut c = characters.chars();
    let (Some(k), None, Some(c), None) = (k.next(), k.next(), c.next(), c.next()) else {
        return false;
    };
    matches!(
        (k, c),
        (BACKSPACE, DEL) | ('\r', KEYPAD_ENTER) | ('\t', BACK_TAB)
    )
}

/// Whether a key event fires `eq`: the same key, and exactly the same
/// Command / Shift / Option / Control (caps lock and the rest ignored) —
/// which must include Command (see the module docs).
pub fn chord_fires(eq: &KeyEquivalent, characters: &str, flags: u64) -> bool {
    use modifier_flags::{COMMAND, CONTROL, OPTION, SHIFT};
    let chord = flags & (COMMAND | SHIFT | CONTROL | OPTION);
    eq.modifier_flags & COMMAND != 0
        && chord == eq.modifier_flags
        && !characters.is_empty()
        && same_key(&eq.key, characters)
}

/// The first of `shown` (the items a built menu shows, in order) with its own
/// [`MenuItemModel::shortcut`] that the key event fires and whose visibility
/// and enabled gates pass now.  Items without a shortcut of their own are
/// left to [`super::match_key_equivalent`].
pub fn match_shortcut(
    shown: &[MenuItemModel],
    characters: &str,
    flags: u64,
) -> Option<MenuItemModel> {
    shown
        .iter()
        .find(|item| {
            item.shortcut.is_some()
                && key_equivalent(item).is_some_and(|eq| chord_fires(&eq, characters, flags))
                && item.is_visible.as_ref().is_none_or(|gate| gate())
                && is_enabled(item)
        })
        .cloned()
}
