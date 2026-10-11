//! Open menu state and event-side behavior.
//!
//! The state keeps only interaction data. Item trees stay in the model so
//! callers can rebuild or reuse menus without carrying transient hover state.

use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::geometry::{Point, Size};

use super::fit_width::{FitMeasure, MenuWidth, FIT_MIN_W};
use super::geometry::{
    hit_test, item_at_path, metrics_with_row_h, stack_layout_placed, MenuHit, MenuMetrics,
    PopupLayout, ROW_H,
};
use super::model::{MenuEntry, MenuSelection};
use super::overflow::StackPlacement;

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
    /// Popup width policy; see [`MenuWidth`].
    width: MenuWidth,
    /// Font the [`MenuWidth::FitContent`] policy measures rows with.  Until
    /// one is set a fitted popup lays out at the fixed width.
    fit_measure: Option<FitMeasure>,
    /// Desktop floor of a fitted popup's width (logical px), applied to the
    /// root and every submenu level alike.  Defaults to [`FIT_MIN_W`].
    min_width: f64,
    /// Desktop item-row height (logical px); see [`super::MenuStyle::row_h`].
    row_h: f64,
    /// Height of the bar title the menu opened from (`0.0` for a point), so
    /// a menu that flips upward clears it; see [`Self::set_anchor_span`].
    anchor_span: f64,
    /// Root position of the space this menu's geometry is in, when known;
    /// see [`Self::set_space_origin`].
    space_origin: Option<Point>,
    /// Scroll offset of each scrolling level, keyed by its path prefix
    /// (`state_scroll.rs`).  Cleared on open and close.
    pub(super) scroll_offsets: Vec<(Vec<usize>, f64)>,
    /// A scroll bar drag in progress (`state_scroll.rs`).
    scroll_drag: Option<scroll::ScrollDrag>,
    /// Each level's safe triangle toward its open submenu (`state_aim.rs`).
    aim: aim::AimWedges,
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
            width: MenuWidth::Fixed,
            fit_measure: None,
            min_width: FIT_MIN_W,
            row_h: ROW_H,
            anchor_span: 0.0,
            space_origin: None,
            scroll_offsets: Vec::new(),
            scroll_drag: None,
            aim: aim::AimWedges::default(),
        }
    }
}

impl PopupMenuState {
    pub fn open_at(&mut self, anchor: Point, anchor_kind: MenuAnchorKind) {
        self.anchor = anchor;
        self.anchor_kind = anchor_kind;
        self.open = true;
        self.open_path.clear();
        self.scroll_offsets.clear();
        self.scroll_drag = None;
        self.aim.clear();
        self.hover_path = None;
        self.suppress_next_mouse_up = false;
        self.activate_on_mouse_up = false;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.open_path.clear();
        self.scroll_offsets.clear();
        self.scroll_drag = None;
        self.aim.clear();
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

    /// Choose how wide the popup panels are (default [`MenuWidth::Fixed`]).
    pub fn set_width(&mut self, width: MenuWidth) {
        self.width = width;
    }

    pub fn width(&self) -> MenuWidth {
        self.width
    }

    /// Set the narrowest a [`MenuWidth::FitContent`] popup may be, in desktop
    /// logical px (touch-grown with the other metrics).  The default is
    /// [`FIT_MIN_W`]; `0.0` drops the floor so each panel is exactly its
    /// widest row.  One value covers the root and every submenu.  Has no
    /// effect on [`MenuWidth::Fixed`] popups.
    pub fn set_min_width(&mut self, min_width: f64) {
        self.min_width = min_width;
    }

    pub fn min_width(&self) -> f64 {
        self.min_width
    }

    /// Set the desktop item-row height (logical px, touch-floored).  The
    /// default is [`ROW_H`].
    pub fn set_row_height(&mut self, row_h: f64) {
        self.row_h = row_h;
    }

    pub fn row_height(&self) -> f64 {
        self.row_h
    }

    /// Set the font, size and row style a [`MenuWidth::FitContent`] popup
    /// measures with.  `PopupMenu` / `MenuBar` keep this in step with the
    /// font they paint with, so hit-testing sees the painted widths.
    pub fn set_fit_measure(&mut self, measure: FitMeasure) {
        self.fit_measure = Some(measure);
    }

    pub fn layouts(&self, items: &[MenuEntry], viewport: Size) -> Vec<PopupLayout> {
        if !self.open {
            return Vec::new();
        }
        let m = metrics_with_row_h(self.row_h);
        let scroll = |prefix: &[usize]| self.scroll_offset(prefix);
        let placement = StackPlacement {
            anchor_span: self.anchor_span,
            space_origin: self.space_origin,
            scroll: &scroll,
        };
        match (self.width, &self.fit_measure) {
            (MenuWidth::FitContent, Some(measure)) => stack_layout_placed(
                items,
                self.anchor,
                self.anchor_kind,
                &self.open_path,
                viewport,
                &|level, m| measure.popup_width_with_min(level, m, self.min_width),
                m,
                &placement,
            ),
            _ => stack_layout_placed(
                items,
                self.anchor,
                self.anchor_kind,
                &self.open_path,
                viewport,
                &|_, m: &MenuMetrics| m.menu_w,
                m,
                &placement,
            ),
        }
    }

    /// Set the height of the bar title the menu opens from, so a bar menu
    /// with no room on its preferred side flips past the title rather than
    /// over it.  `0.0` (the default) anchors at a point.
    pub fn set_anchor_span(&mut self, span: f64) {
        self.anchor_span = span;
    }

    /// Tell the menu where the space its anchor and events are given in sits
    /// in root logical coordinates.  With it the window can be mapped into
    /// that space, so a bar menu ([`MenuAnchorKind::Bar`]) flips upward or
    /// scrolls instead of hanging off the bottom.  [`super::MenuBar`] keeps
    /// it current from its overlay paint.
    pub fn set_space_origin(&mut self, origin: Option<Point>) {
        self.space_origin = origin;
    }

    pub fn space_origin(&self) -> Option<Point> {
        self.space_origin
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
        if let Some(result) = self.handle_scroll_bar(items, event, viewport) {
            return (result, MenuResponse::None);
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
            // The user switched to another application: close, as a native
            // menu does (agg-sharp's `PopupPlacement` / `PopupWidget` close on
            // `SystemWindow.Deactivated`). Focus is not consulted — the host
            // keeps it across an app switch. `Closed` lets the host run the
            // same teardown as any other dismissal.
            Event::WindowDeactivated => {
                self.close();
                crate::animation::request_draw();
                (EventResult::Consumed, MenuResponse::Closed)
            }
            // Any non-left press dismisses — the desktop convention.
            // Right-clicking with a menu open closes it instead of
            // leaving it hanging over whatever context menu that press
            // is about to raise. Consumed so the press only dismisses:
            // it must not also activate the item underneath it.
            Event::MouseDown { .. } => {
                self.close();
                crate::animation::request_draw();
                (EventResult::Consumed, MenuResponse::Closed)
            }
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
                    let result = self.handle_key(items, key.clone());
                    if matches!(key, Key::ArrowDown | Key::ArrowUp) {
                        self.reveal_hover(items, viewport);
                    }
                    result
                }
            }
            Event::MouseWheel { pos, delta_y, .. } => (
                self.handle_wheel(items, *pos, *delta_y, viewport),
                MenuResponse::None,
            ),
            _ => (EventResult::Ignored, MenuResponse::None),
        }
    }

    pub fn update_hover(&mut self, items: &[MenuEntry], pos: Point, viewport: Size) -> bool {
        // Touch has no hover concept: a tap synthesises a MouseMove at the tap
        // point right before the MouseDown.  Doing hover work here — especially
        // OPENING a submenu via `open_path` — means the follow-up MouseDown
        // hit-tests against the just-opened submenu and, on a narrow viewport
        // where the submenu overlaps its parent, lands on (and activates) the
        // submenu's first child instead of opening the submenu.  On touch,
        // submenus open only on the explicit tap in `handle_left_down`; leave
        // `hover_path`/`open_path` untouched here (clear any stale hover).
        if is_touch_synthesized() {
            return self.set_hover_path(None);
        }
        let layouts = self.layouts(items, viewport);
        let next_hover = match hit_test(&layouts, pos) {
            Some(MenuHit::Item(path)) => {
                if let Some(item) = item_at_path(items, &path) {
                    // Widget rows take their own hover; the menu does not
                    // highlight them.
                    if item.widget_row.is_some() {
                        if !self.open_path.starts_with(&path) {
                            self.open_path.truncate(path.len().saturating_sub(1));
                        }
                        return self.set_hover_path(None);
                    }
                    // agg-sharp routes no hover to a disabled row, so the
                    // highlight and any open submenu stay where they were
                    // (`PopupMenuHoverTests.HoveringADisabledRowDoesNotMoveTheHighlight`).
                    if !item.enabled {
                        return false;
                    }
                    if !self.hover_row(&layouts, &path, item.has_submenu(), pos) {
                        return false;
                    }
                }
                Some(path)
            }
            _ => None,
        };
        // (Touch is handled by the early return above — only desktop hover
        // reaches here.)
        if self.hover_path != next_hover {
            self.hover_path = next_hover;
            true
        } else {
            false
        }
    }

    /// agg-sharp `PopupMenu.OnRowHover` for the enabled row at `path`:
    /// opens its submenu or closes the one a sibling had open, unless the
    /// pointer is crossing the row on its way into that open submenu
    /// (`state_aim.rs`).  Returns false when the row is only crossed, and
    /// the highlight must stay put.
    fn hover_row(
        &mut self,
        layouts: &[PopupLayout],
        path: &[usize],
        has_submenu: bool,
        pos: Point,
    ) -> bool {
        let level = &path[..path.len() - 1];
        // The pointer has arrived in this level, so the wedge the level
        // above was holding open for it is spent.
        if let Some((_, parent)) = level.split_last() {
            self.aim.forget(parent);
        }
        let open_row = (self.open_path.len() > level.len() && self.open_path.starts_with(level))
            .then(|| self.open_path[..=level.len()].to_vec());
        if open_row.as_deref() == Some(path) {
            // Back on the row whose submenu is up (on the way out of it):
            // leave the chain below it open.
            self.aim.remember(level, pos);
            return true;
        }
        if let Some(open_row) = &open_row {
            let sub = layouts.iter().find(|l| &l.path_prefix == open_row);
            if sub.is_some_and(|sub| self.aim.aiming_at(level, pos, sub.rect)) {
                return false;
            }
        }
        self.aim.remember(level, pos);
        if has_submenu {
            self.open_path = path.to_vec();
        } else {
            self.open_path.truncate(level.len());
        }
        true
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
        // agg-sharp `RadioMenuItem`: choosing the row that is already the
        // selected radio does nothing at all (no setter call, no state
        // change).  The row still closes a menu it would close.
        let reselects_radio = item_at_path(items, path)
            .is_some_and(|item| item.selection == MenuSelection::Radio { selected: true });
        if reselects_radio {
            if !close_on_activate || !self.open {
                return (EventResult::Consumed, MenuResponse::None);
            }
            self.close();
            self.suppress_next_mouse_up = suppress_mouse_up;
            crate::animation::request_draw();
            return (EventResult::Consumed, MenuResponse::Closed);
        }
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
                    self.enter_submenu(items, path);
                }
                (EventResult::Consumed, MenuResponse::None)
            }
            Key::ArrowLeft => {
                // agg-sharp `PopupMenu.OnKeyDown`: Left closes the deepest
                // open submenu and highlights the row that opened it.  In a
                // top-level menu there is nothing to back out of, so the
                // highlight stays put (a menu bar walks its menus instead).
                if !self.open_path.is_empty() {
                    self.hover_path = Some(self.open_path.clone());
                    self.open_path.pop();
                    crate::animation::request_draw();
                }
                (EventResult::Consumed, MenuResponse::None)
            }
            Key::Enter | Key::Char(' ') => {
                if let Some(path) = self.hover_path.clone() {
                    // On a submenu row Enter / Space open it like Right
                    // (agg-sharp: the row's click is what opens it).
                    if self.enter_submenu(items, path.clone()) {
                        return (EventResult::Consumed, MenuResponse::None);
                    }
                    if let Some(item) = item_at_path(items, &path).filter(|item| item.enabled) {
                        let action = item.action.clone();
                        let close_on_activate = item.close_on_activate;
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
                (EventResult::Consumed, MenuResponse::None)
            }
            _ => (EventResult::Ignored, MenuResponse::None),
        }
    }

    /// Open the submenu of the enabled item at `path`, as agg-sharp's
    /// `SubMenuItemButton.OpenSubMenu` does: showing the submenu focuses the
    /// submenu panel rather than a row, so no row is highlighted (the opener
    /// keeps its open fill through `open_path`) and the next Up / Down steps
    /// from nothing inside it.  Returns `false`, and changes nothing, when
    /// `path` is not an enabled item with a submenu.  Shared by Right and
    /// Enter / Space.
    fn enter_submenu(&mut self, items: &[MenuEntry], path: Vec<usize>) -> bool {
        if !item_at_path(items, &path).is_some_and(|item| item.enabled && item.has_submenu()) {
            return false;
        }
        self.open_path = path;
        self.hover_path = None;
        crate::animation::request_draw();
        true
    }

    fn step_hover(&mut self, items: &[MenuEntry], delta: isize) {
        let level_items = items_at_path(items, &self.open_path).unwrap_or(items);
        let enabled: Vec<usize> = level_items
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| match entry {
                MenuEntry::Item(item) if item.enabled && item.widget_row.is_none() => Some(idx),
                _ => None,
            })
            .collect();
        if enabled.is_empty() {
            return;
        }
        // Only a highlighted row of the stepped level counts as "current".
        // A hover-opened submenu leaves the highlight on its opener one level
        // up; agg-sharp has focused the shown submenu panel by then, so the
        // step starts from nothing inside it (`PopupMenu.MoveHighlight`).
        let current = self
            .hover_path
            .as_ref()
            .and_then(|path| path.split_last())
            .filter(|(_, parent)| *parent == self.open_path.as_slice())
            .and_then(|(idx, _)| enabled.iter().position(|candidate| candidate == idx));
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

#[path = "state_aim.rs"]
mod aim;
#[path = "state_scroll.rs"]
mod scroll;

#[cfg(test)]
#[path = "state_keyboard_tests.rs"]
mod keyboard_tests;
