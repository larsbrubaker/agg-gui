//! Item-tree path helpers for [`PopupMenuState`]: resolving a path of row
//! indices to a level or an item, finding the item a shortcut key fires, and
//! applying a check / radio toggle at a path.
//!
//! Split out of `state.rs` (the event-side behaviour) because these are pure
//! functions over `MenuEntry` trees that hold no interaction state.

use crate::event::{Key, Modifiers};

use super::super::geometry::item_at_path;
use super::super::model::{MenuEntry, MenuSelection};

pub(super) fn items_at_path<'a>(items: &'a [MenuEntry], path: &[usize]) -> Option<&'a [MenuEntry]> {
    let mut current = items;
    for &idx in path {
        current = &item_at_path(current, &[idx])?.submenu;
    }
    Some(current)
}

pub(super) fn toggle_selection_at_path(items: &mut [MenuEntry], path: &[usize]) {
    let Some(selection) = item_at_path(items, path).map(|item| item.selection) else {
        return;
    };
    match selection {
        MenuSelection::Check { selected } => {
            if let Some(item) = item_at_path_mut(items, path) {
                item.selection = MenuSelection::Check {
                    selected: !selected,
                };
            }
        }
        MenuSelection::Radio { .. } => {
            let Some((&idx, parent_path)) = path.split_last() else {
                return;
            };
            let Some(parent) = entries_at_path_mut(items, parent_path) else {
                return;
            };
            for entry in parent.iter_mut() {
                if let MenuEntry::Item(item) = entry {
                    if matches!(item.selection, MenuSelection::Radio { .. }) {
                        item.selection = MenuSelection::Radio { selected: false };
                    }
                }
            }
            if let Some(MenuEntry::Item(item)) = parent.get_mut(idx) {
                item.selection = MenuSelection::Radio { selected: true };
            }
        }
        MenuSelection::None => {}
    }
}

fn item_at_path_mut<'a>(
    items: &'a mut [MenuEntry],
    path: &[usize],
) -> Option<&'a mut super::super::model::MenuItem> {
    let (&idx, rest) = path.split_first()?;
    let entry = items.get_mut(idx)?;
    match entry {
        MenuEntry::Item(item) => {
            if rest.is_empty() {
                Some(item)
            } else {
                item_at_path_mut(&mut item.submenu, rest)
            }
        }
        MenuEntry::Separator => None,
    }
}

fn entries_at_path_mut<'a>(
    items: &'a mut [MenuEntry],
    path: &[usize],
) -> Option<&'a mut [MenuEntry]> {
    if path.is_empty() {
        return Some(items);
    }
    let (&idx, rest) = path.split_first()?;
    match items.get_mut(idx)? {
        MenuEntry::Item(item) => entries_at_path_mut(&mut item.submenu, rest),
        MenuEntry::Separator => None,
    }
}

pub(super) fn shortcut_path(
    items: &[MenuEntry],
    key: &Key,
    modifiers: Modifiers,
) -> Option<Vec<usize>> {
    for (idx, entry) in items.iter().enumerate() {
        let MenuEntry::Item(item) = entry else {
            continue;
        };
        if item.enabled
            && item
                .accelerator
                .is_some_and(|accelerator| accelerator.matches(key, modifiers))
            && item.action.is_some()
        {
            return Some(vec![idx]);
        }
        if let Some(mut path) = shortcut_path(&item.submenu, key, modifiers) {
            path.insert(0, idx);
            return Some(path);
        }
    }
    None
}
