//! Open menu state and event-side behavior.
//!
//! The state keeps only interaction data. Item trees stay in the model so
//! callers can rebuild or reuse menus without carrying transient hover state.

use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::geometry::{Point, Size};

use super::geometry::{hit_test, item_at_path, stack_layout, MenuHit, PopupLayout};
use super::model::{MenuEntry, MenuSelection};

/// Wall-clock window during which a touch event still classifies follow-up
/// mouse events as touch-synthesised.  Mirrors the constant in the menu
/// widget; duplicated here so this module stays standalone-testable
/// instead of pulling in the widget impl.
const TOUCH_SYNTH_WINDOW_MS: u128 = 50;

fn is_touch_synthesized() -> bool {
    crate::touch_state::last_touch_event_age()
        .map(|d| d.as_millis() < TOUCH_SYNTH_WINDOW_MS)
        .unwrap_or(false)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAnchorKind {
    Context,
    /// Top menu bar — anchor is the bar item's BOTTOM edge; popup
    /// opens DOWNWARD (extending toward smaller y in Y-up).
    Bar,
    /// Bottom menu bar — anchor is the bar item's TOP edge; popup
    /// opens UPWARD (extending toward larger y in Y-up). Used by
    /// callers that position the menu bar across the bottom of
    /// the viewport, where opening downward would clip the popup
    /// against the viewport floor.
    BottomBar,
}

#[derive(Clone, Debug)]
pub struct PopupMenuState {
    pub anchor: Point,
    pub anchor_kind: MenuAnchorKind,
    pub open: bool,
    pub open_path: Vec<usize>,
    pub hover_path: Option<Vec<usize>>,
    suppress_next_mouse_up: bool,
    activate_on_mouse_up: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuResponse {
    None,
    Action(String),
    Closed,
}

impl Default for PopupMenuState {
    fn default() -> Self {
        Self {
            anchor: Point::ORIGIN,
            anchor_kind: MenuAnchorKind::Context,
            open: false,
            open_path: Vec::new(),
            hover_path: None,
            suppress_next_mouse_up: false,
            activate_on_mouse_up: false,
        }
    }
}

impl PopupMenuState {
    pub fn open_at(&mut self, anchor: Point, anchor_kind: MenuAnchorKind) {
        self.anchor = anchor;
        self.anchor_kind = anchor_kind;
        self.open = true;
        self.open_path.clear();
        self.hover_path = None;
        self.suppress_next_mouse_up = false;
        self.activate_on_mouse_up = false;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.open_path.clear();
        self.hover_path = None;
        self.activate_on_mouse_up = false;
    }

    pub fn arm_mouse_up_activation(&mut self) {
        self.activate_on_mouse_up = true;
    }

    pub fn is_mouse_up_activation_armed(&self) -> bool {
        self.activate_on_mouse_up
    }

    pub fn handle_shortcut(
        &mut self,
        items: &mut [MenuEntry],
        key: &Key,
        modifiers: Modifiers,
    ) -> MenuResponse {
        let Some(path) = shortcut_path(items, key, modifiers) else {
            return MenuResponse::None;
        };
        let Some(item) = item_at_path(items, &path) else {
            return MenuResponse::None;
        };
        let Some(action) = item.action.clone() else {
            return MenuResponse::None;
        };
        let close_on_activate = item.close_on_activate;
        let (_, response) = self.activate_action(items, &path, action, close_on_activate, false);
        response
    }

    pub fn should_suppress_mouse_up(&self) -> bool {
        self.suppress_next_mouse_up
    }

    pub fn take_suppress_mouse_up(&mut self) -> bool {
        let suppress = self.suppress_next_mouse_up;
        self.suppress_next_mouse_up = false;
        suppress
    }

    pub fn layouts(&self, items: &[MenuEntry], viewport: Size) -> Vec<PopupLayout> {
        if self.open {
            stack_layout(
                items,
                self.anchor,
                self.anchor_kind,
                &self.open_path,
                viewport,
            )
        } else {
            Vec::new()
        }
    }

    pub fn handle_event(
        &mut self,
        items: &mut [MenuEntry],
        event: &Event,
        viewport: Size,
    ) -> (EventResult, MenuResponse) {
        if !self.open {
            return (EventResult::Ignored, MenuResponse::None);
        }
        match event {
            Event::MouseMove { pos } => {
                let changed = self.update_hover(items, *pos, viewport);
                if changed {
                    crate::animation::request_draw_without_invalidation();
                }
                (EventResult::Consumed, MenuResponse::None)
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => self.handle_left_down(items, *pos, viewport),
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
                ..
            } if self.activate_on_mouse_up => {
                self.activate_on_mouse_up = false;
                self.handle_release_activation(items, *pos, viewport)
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } if self.take_suppress_mouse_up() => (EventResult::Consumed, MenuResponse::None),
            Event::KeyDown { key, modifiers } => {
                let response = self.handle_shortcut(items, key, *modifiers);
                if response != MenuResponse::None {
                    (EventResult::Consumed, response)
                } else {
                    self.handle_key(items, key.clone())
                }
            }
            _ => (EventResult::Ignored, MenuResponse::None),
        }
    }

    pub fn update_hover(&mut self, items: &[MenuEntry], pos: Point, viewport: Size) -> bool {
        let layouts = self.layouts(items, viewport);
        let next_hover = match hit_test(&layouts, pos) {
            Some(MenuHit::Item(path)) => {
                if let Some(item) = item_at_path(items, &path) {
                    if !item.enabled {
                        if !self.open_path.starts_with(&path) {
                            self.open_path.truncate(path.len().saturating_sub(1));
                        }
                        return self.set_hover_path(None);
                    }
                    if item.enabled && item.has_submenu() {
                        self.open_path = path.clone();
                    } else if !self.open_path.starts_with(&path) {
                        self.open_path.truncate(path.len().saturating_sub(1));
                    }
                }
                Some(path)
            }
            _ => None,
        };
        // Touch-synth MouseMove arrives during a tap; treat hover as None
        // so the dropped finger doesn't leave a hover panel behind on the
        // popup row after the tap closes the menu.
        let next_hover = if is_touch_synthesized() {
            None
        } else {
            next_hover
        };
        if self.hover_path != next_hover {
            self.hover_path = next_hover;
            true
        } else {
            false
        }
    }

    fn set_hover_path(&mut self, hover_path: Option<Vec<usize>>) -> bool {
        if self.hover_path != hover_path {
            self.hover_path = hover_path;
            true
        } else {
            false
        }
    }

    fn handle_left_down(
        &mut self,
        items: &mut [MenuEntry],
        pos: Point,
        viewport: Size,
    ) -> (EventResult, MenuResponse) {
        let layouts = self.layouts(items, viewport);
        match hit_test(&layouts, pos) {
            Some(MenuHit::Item(path)) => {
                let Some(item) = item_at_path(items, &path) else {
                    return (EventResult::Consumed, MenuResponse::None);
                };
                let enabled = item.enabled;
                let has_submenu = item.has_submenu();
                let action = item.action.clone();
                let close_on_activate = item.close_on_activate;
                if !enabled {
                    self.hover_path = None;
                    return (EventResult::Consumed, MenuResponse::None);
                }
                self.hover_path = Some(path.clone());
                if has_submenu {
                    self.open_path = path;
                    crate::animation::request_draw();
                    (EventResult::Consumed, MenuResponse::None)
                } else if let Some(action) = action {
                    self.activate_action(items, &path, action, close_on_activate, true)
                } else {
                    (EventResult::Consumed, MenuResponse::None)
                }
            }
            Some(MenuHit::Panel) => (EventResult::Consumed, MenuResponse::None),
            None => {
                self.close();
                self.suppress_next_mouse_up = true;
                crate::animation::request_draw();
                (EventResult::Consumed, MenuResponse::Closed)
            }
        }
    }

    fn handle_release_activation(
        &mut self,
        items: &mut [MenuEntry],
        pos: Point,
        viewport: Size,
    ) -> (EventResult, MenuResponse) {
        let layouts = self.layouts(items, viewport);
        match hit_test(&layouts, pos) {
            Some(MenuHit::Item(path)) => {
                self.hover_path = Some(path.clone());
                let Some(item) = item_at_path(items, &path) else {
                    return (EventResult::Consumed, MenuResponse::None);
                };
                let enabled = item.enabled;
                let has_submenu = item.has_submenu();
                let action = item.action.clone();
                let close_on_activate = item.close_on_activate;
                if !enabled || has_submenu {
                    return (EventResult::Consumed, MenuResponse::None);
                }
                if let Some(action) = action {
                    self.activate_action(items, &path, action, close_on_activate, false)
                } else {
                    (EventResult::Consumed, MenuResponse::None)
                }
            }
            Some(MenuHit::Panel) | None => (EventResult::Consumed, MenuResponse::None),
        }
    }

    fn activate_action(
        &mut self,
        items: &mut [MenuEntry],
        path: &[usize],
        action: String,
        close_on_activate: bool,
        suppress_mouse_up: bool,
    ) -> (EventResult, MenuResponse) {
        toggle_selection_at_path(items, path);
        if close_on_activate {
            self.close();
            self.suppress_next_mouse_up = suppress_mouse_up;
        }
        crate::animation::request_draw();
        (EventResult::Consumed, MenuResponse::Action(action))
    }

    fn handle_key(&mut self, items: &mut [MenuEntry], key: Key) -> (EventResult, MenuResponse) {
        match key {
            Key::Escape => {
                self.close();
                crate::animation::request_draw();
                (EventResult::Consumed, MenuResponse::Closed)
            }
            Key::ArrowDown => {
                self.step_hover(items, 1);
                (EventResult::Consumed, MenuResponse::None)
            }
            Key::ArrowUp => {
                self.step_hover(items, -1);
                (EventResult::Consumed, MenuResponse::None)
            }
            Key::ArrowRight => {
                if let Some(path) = self.hover_path.clone() {
                    if item_at_path(items, &path).is_some_and(|item| item.has_submenu()) {
                        self.open_path = path;
                        crate::animation::request_draw();
                    }
                }
                (EventResult::Consumed, MenuResponse::None)
            }
            Key::ArrowLeft => {
                self.open_path.pop();
                self.hover_path = self.open_path.last().map(|_| self.open_path.clone());
                crate::animation::request_draw();
                (EventResult::Consumed, MenuResponse::None)
            }
            Key::Enter | Key::Char(' ') => {
                if let Some(path) = self.hover_path.clone() {
                    if let Some(item) = item_at_path(items, &path) {
                        let enabled = item.enabled;
                        let has_submenu = item.has_submenu();
                        let action = item.action.clone();
                        let close_on_activate = item.close_on_activate;
                        if enabled && has_submenu {
                            self.open_path = path;
                        } else if enabled {
                            if let Some(action) = action {
                                return self.activate_action(
                                    items,
                                    &path,
                                    action,
                                    close_on_activate,
                                    false,
                                );
                            }
                        }
                    }
                }
                (EventResult::Consumed, MenuResponse::None)
            }
            _ => (EventResult::Ignored, MenuResponse::None),
        }
    }

    fn step_hover(&mut self, items: &[MenuEntry], delta: isize) {
        let level_items = items_at_path(items, &self.open_path).unwrap_or(items);
        let enabled: Vec<usize> = level_items
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| match entry {
                MenuEntry::Item(item) if item.enabled => Some(idx),
                _ => None,
            })
            .collect();
        if enabled.is_empty() {
            return;
        }
        let current = self
            .hover_path
            .as_ref()
            .and_then(|path| path.last().copied())
            .and_then(|idx| enabled.iter().position(|candidate| *candidate == idx));
        let base = current
            .map(|idx| idx as isize)
            .unwrap_or(if delta > 0 { -1 } else { 0 });
        let next = (base + delta).rem_euclid(enabled.len() as isize) as usize;
        let mut path = self.open_path.clone();
        path.push(enabled[next]);
        self.hover_path = Some(path);
        crate::animation::request_draw();
    }
}

fn items_at_path<'a>(items: &'a [MenuEntry], path: &[usize]) -> Option<&'a [MenuEntry]> {
    let mut current = items;
    for &idx in path {
        current = &item_at_path(current, &[idx])?.submenu;
    }
    Some(current)
}

fn toggle_selection_at_path(items: &mut [MenuEntry], path: &[usize]) {
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
) -> Option<&'a mut super::model::MenuItem> {
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

fn shortcut_path(items: &[MenuEntry], key: &Key, modifiers: Modifiers) -> Option<Vec<usize>> {
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
