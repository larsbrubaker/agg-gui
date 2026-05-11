//! Widget adapters for reusable menus.
//!
//! `ContextMenu` is a small controller that other widgets can embed, while
//! `MenuBar` is a visible widget for top-level menus.

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::font_settings;
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::{current_viewport, Widget};

use super::geometry::{contains, item_at_path, BAR_H};
use super::model::MenuEntry;
use super::paint::{paint_menu_bar_button, paint_popup_stack, MenuStyle};
use super::state::{MenuAnchorKind, MenuResponse, PopupMenuState};

/// Mouse events synthesised from a touch tap arrive within a few
/// milliseconds of the corresponding `touchstart`/`touchend`.  Allow a
/// generous window (50 ms) so a busy frame doesn't accidentally
/// classify a synthesised event as a desktop click.
const TOUCH_SYNTH_WINDOW_MS: u128 = 50;

fn is_touch_synthesized() -> bool {
    crate::touch_state::last_touch_event_age()
        .map(|d| d.as_millis() < TOUCH_SYNTH_WINDOW_MS)
        .unwrap_or(false)
}

#[derive(Clone)]
pub struct PopupMenu {
    pub items: Vec<MenuEntry>,
    pub state: PopupMenuState,
    pub style: MenuStyle,
}

impl PopupMenu {
    pub fn new(items: Vec<MenuEntry>) -> Self {
        Self {
            items,
            state: PopupMenuState::default(),
            style: MenuStyle::default(),
        }
    }

    pub fn open_at(&mut self, pos: Point) {
        self.state.open_at(pos, MenuAnchorKind::Context);
    }

    pub fn close(&mut self) {
        self.state.close();
    }

    pub fn is_open(&self) -> bool {
        self.state.open
    }

    pub fn take_suppress_mouse_up(&mut self) -> bool {
        self.state.take_suppress_mouse_up()
    }

    pub fn handle_event(&mut self, event: &Event, viewport: Size) -> (EventResult, MenuResponse) {
        self.state.handle_event(&mut self.items, event, viewport)
    }

    /// Return `true` if `pos` falls inside any of the popup's currently
    /// laid-out panels (the open menu plus any nested submenus).  Used
    /// by `MenuBar` to detect a mouse-up in "neutral space" — outside
    /// both the menu bar AND the popup body — so the bar can dismiss
    /// the popup without waiting for a follow-up event.
    pub fn body_contains(&self, pos: Point, viewport: Size) -> bool {
        self.state
            .layouts(&self.items, viewport)
            .iter()
            .any(|layout| {
                pos.x >= layout.rect.x
                    && pos.x <= layout.rect.x + layout.rect.width
                    && pos.y >= layout.rect.y
                    && pos.y <= layout.rect.y + layout.rect.height
            })
    }

    pub fn handle_shortcut(&mut self, key: &Key, modifiers: Modifiers) -> MenuResponse {
        self.state.handle_shortcut(&mut self.items, key, modifiers)
    }

    pub fn paint(&self, ctx: &mut dyn DrawCtx, font: Arc<Font>, font_size: f64, viewport: Size) {
        let layouts = self.state.layouts(&self.items, viewport);
        paint_popup_stack(
            ctx,
            font,
            font_size,
            &self.items,
            &self.state,
            &layouts,
            &self.style,
        );
    }
}

pub struct MenuBar {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    font_size: f64,
    menus: Vec<TopMenu>,
    open_index: Option<usize>,
    hover_index: Option<usize>,
    popup: PopupMenu,
    on_action: Box<dyn FnMut(&str)>,
    /// Top-menu index whose hover highlight should NOT paint until the
    /// cursor leaves it.  Set when the user closes a popup by clicking
    /// the currently-open top menu's bar item — without this the bar
    /// would keep showing the hover-tinted background after the close
    /// (the cursor is still over the bar item) and read as "still
    /// selected" to the user.  Cleared in `set_hover_index` when the
    /// hovered idx changes to anything else.
    suppress_hover_for: Option<usize>,
    /// When `true`, [`Widget::layout`] returns the tight content width
    /// (sum of menu-button widths) instead of the full available width.
    /// Set via [`MenuBar::with_fit_width`] when the bar shares a FlexRow
    /// with right-aligned chrome (e.g. project title, About button) and
    /// shouldn't claim every spare pixel.
    fit_width: bool,
}

pub struct TopMenu {
    pub label: String,
    pub items: Vec<MenuEntry>,
    rect: Rect,
}

impl TopMenu {
    pub fn new(label: impl Into<String>, items: Vec<MenuEntry>) -> Self {
        Self {
            label: label.into(),
            items,
            rect: Rect::default(),
        }
    }
}

impl MenuBar {
    pub fn new(
        font: Arc<Font>,
        menus: Vec<TopMenu>,
        on_action: impl FnMut(&str) + 'static,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            font_size: 14.0,
            menus,
            open_index: None,
            hover_index: None,
            popup: PopupMenu::new(Vec::new()),
            on_action: Box::new(on_action),
            suppress_hover_for: None,
            fit_width: false,
        }
    }

    /// Opt into tight-width sizing — `Widget::layout` will report the
    /// summed menu-button width rather than the full available width.
    /// Use when the MenuBar is hosted inside a `FlexRow` with sibling
    /// chrome on the right (project title, status indicators, etc.)
    /// that needs to share the same row.
    pub fn with_fit_width(mut self, fit: bool) -> Self {
        self.fit_width = fit;
        self
    }

    pub fn with_font_size(mut self, font_size: f64) -> Self {
        self.font_size = font_size;
        self
    }

    /// Resolve the font used for layout/paint.  Prefers the system-wide
    /// font override so the System window's font picker propagates live;
    /// falls back to the per-instance font otherwise.  Mirrors the
    /// `Label::active_font` pattern.
    fn active_font(&self) -> Arc<Font> {
        font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font))
    }

    fn menu_at(&self, pos: Point) -> Option<usize> {
        self.menus.iter().position(|menu| contains(menu.rect, pos))
    }

    fn open_menu(&mut self, idx: usize) {
        let rect = self.menus[idx].rect;
        self.popup.items = self.menus[idx].items.clone();
        self.popup
            .state
            .open_at(Point::new(rect.x, rect.y), MenuAnchorKind::Bar);
        self.open_index = Some(idx);
        self.hover_index = Some(idx);
        crate::animation::request_draw();
    }

    fn open_menu_for_drag_release(&mut self, idx: usize) {
        self.open_menu(idx);
        self.popup.state.arm_mouse_up_activation();
    }

    fn switch_open_menu(&mut self, delta: isize) -> EventResult {
        let Some(current) = self.open_index else {
            return EventResult::Ignored;
        };
        if self.menus.is_empty() {
            return EventResult::Ignored;
        }
        let len = self.menus.len() as isize;
        let next = (current as isize + delta).rem_euclid(len) as usize;
        self.open_menu(next);
        EventResult::Consumed
    }

    fn should_switch_top_menu(&self, key: &Key) -> bool {
        match key {
            Key::ArrowLeft => self.popup.state.open_path.is_empty(),
            Key::ArrowRight => {
                if !self.popup.state.open_path.is_empty() {
                    return false;
                }
                self.popup
                    .state
                    .hover_path
                    .as_deref()
                    .and_then(|path| item_at_path(&self.popup.items, path))
                    .map_or(true, |item| !item.has_submenu())
            }
            _ => false,
        }
    }

    fn set_hover_index(&mut self, hover: Option<usize>) {
        // Touch devices have no real cursor; the synth-MouseMove fired
        // alongside a touchstart would otherwise paint a hover panel that
        // sticks after the tap (no MouseMove ever leaves the bar to clear
        // it).  Coerce hover to `None` for any input within the touch-synth
        // window so a tap-to-open / tap-to-close cycle leaves no residue.
        let hover = if is_touch_synthesized() { None } else { hover };
        if self.hover_index != hover {
            self.hover_index = hover;
            // `request_draw()` (NOT `_without_invalidation`) — the bar's
            // hover paint lives inside the parent Window's retained
            // backbuffer, so the cache must invalidate or the next paint
            // composites a stale bitmap.  The epoch bump in `request_draw`
            // is what `dispatch_event` reads to mark the ancestor path
            // dirty even when this MouseMove returns `Ignored`.
            crate::animation::request_draw();
        }
        // Cursor moved to a different top-menu (or off any) — clear
        // the post-close hover suppression so the next genuine hover
        // re-enters with the usual highlight.
        if self.suppress_hover_for != hover {
            self.suppress_hover_for = None;
        }
    }
}

impl Widget for MenuBar {
    fn type_name(&self) -> &'static str {
        "MenuBar"
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        let mut x = 0.0;
        for menu in &mut self.menus {
            let width = (menu.label.chars().count() as f64 * 8.0 + 22.0).max(52.0);
            menu.rect = Rect::new(x, 0.0, width, BAR_H);
            x += width;
        }
        // `fit_width` mode reports the tight content width so a parent
        // FlexRow can place sibling widgets to the right of the bar.
        // Default mode keeps the historical behaviour (full available
        // width — the bar paints its background across the whole row).
        let report_w = if self.fit_width { x } else { available.width };
        Size::new(report_w, BAR_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_font(self.active_font());
        ctx.set_font_size(self.font_size);
        let v = ctx.visuals();
        ctx.set_fill_color(v.top_bar_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, BAR_H);
        ctx.fill();
        for (idx, menu) in self.menus.iter().enumerate() {
            // After a click-to-close-toggle, the cursor is still over
            // the bar item so `hover_index` still points at it —
            // suppress the hover highlight until the cursor moves off
            // and back on, so the closed menu doesn't read as "still
            // selected".
            let hovered = self.hover_index == Some(idx) && self.suppress_hover_for != Some(idx);
            paint_menu_bar_button(
                ctx,
                menu.rect,
                &menu.label,
                self.open_index == Some(idx),
                hovered,
            );
        }
    }

    fn hit_test_global_overlay(&self, _local_pos: Point) -> bool {
        self.popup.is_open()
    }

    fn has_active_modal(&self) -> bool {
        self.popup.is_open()
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseMove { pos } = event {
            let hovered = self.menu_at(*pos);
            self.set_hover_index(hovered);
            // Hover-switch a different open menu while a popup is open.
            // Suppressed when this MouseMove was synthesised by the
            // touch shell from a touchstart — on mobile the synth move
            // arrives at the tap position immediately followed by a
            // synth MouseDown at the same point; switching the open
            // menu here would make that MouseDown look like a click on
            // the currently-open menu and toggle-close the popup the
            // user just tapped to open.  On desktop the
            // `last_touch_event_age` is `None` (or very large), so
            // hover-switch works as before.
            let from_touch = is_touch_synthesized();
            if self.popup.is_open() && !from_touch {
                if let Some(idx) = hovered {
                    if self.open_index != Some(idx) {
                        let activate_on_release = self.popup.state.is_mouse_up_activation_armed();
                        self.open_menu(idx);
                        if activate_on_release {
                            self.popup.state.arm_mouse_up_activation();
                        }
                    }
                    return EventResult::Consumed;
                }
            }
        }
        if self.popup.is_open() {
            if let Event::KeyDown { key, .. } = event {
                if self.should_switch_top_menu(key) {
                    return match key {
                        Key::ArrowLeft => self.switch_open_menu(-1),
                        Key::ArrowRight => self.switch_open_menu(1),
                        _ => EventResult::Ignored,
                    };
                }
            }
            // Tap-to-switch: when one menu is already open and a
            // MouseDown lands on a DIFFERENT top menu's bar, switch
            // directly.  Without this, the popup handler would see the
            // MouseDown as outside-the-popup-body and close the menu,
            // leaving the user staring at an empty bar.  Clicking the
            // currently-open menu falls through to the popup so it can
            // close (toggle, the desktop convention).
            if let Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } = event
            {
                if let Some(idx) = self.menu_at(*pos) {
                    if self.open_index != Some(idx) {
                        self.open_menu(idx);
                        return EventResult::Consumed;
                    }
                }
            }
            // Drag-release in neutral space cancels.  The user pressed
            // a top menu, dragged off both the bar and the popup body,
            // and let go — the standard menu convention is to dismiss.
            // The popup state's drag-release handler treats outside-
            // popup-body as a no-op (so a mouse-up still on the bar
            // doesn't close), so the bar enforces the cancel here
            // since only the bar knows where its own top-menu rects
            // live.
            if let Event::MouseUp {
                pos,
                button: MouseButton::Left,
                ..
            } = event
            {
                if self.popup.state.is_mouse_up_activation_armed()
                    && self.menu_at(*pos).is_none()
                    && !self.popup.body_contains(*pos, current_viewport())
                {
                    self.popup.close();
                    self.open_index = None;
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
            }
            let (result, response) = self.popup.handle_event(event, current_viewport());
            if let MenuResponse::Action(action) = response {
                if let Some(idx) = self.open_index {
                    self.menus[idx].items = self.popup.items.clone();
                }
                (self.on_action)(&action);
                if !self.popup.is_open() {
                    self.open_index = None;
                }
            } else if matches!(response, MenuResponse::Closed) {
                self.open_index = None;
                // Suppress the hover highlight on the menu the cursor
                // is still over — without this, click-to-close-toggle
                // leaves the bar item painted in the hover tint and
                // reads as "still selected".  Cleared once the cursor
                // moves to a different top-menu (or off the bar).
                self.suppress_hover_for = self.hover_index;
            }
            if result == EventResult::Consumed {
                return result;
            }
        }
        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(idx) = self.menu_at(*pos) {
                    self.open_menu_for_drag_release(idx);
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseMove { .. } => EventResult::Ignored,
            _ => EventResult::Ignored,
        }
    }

    fn on_unconsumed_key(&mut self, key: &Key, modifiers: Modifiers) -> EventResult {
        let response = if self.popup.is_open() {
            self.popup.handle_shortcut(key, modifiers)
        } else {
            self.menus
                .iter_mut()
                .find_map(|menu| {
                    let mut popup = PopupMenu::new(menu.items.clone());
                    match popup.handle_shortcut(key, modifiers) {
                        MenuResponse::Action(action) => {
                            menu.items = popup.items;
                            Some(action)
                        }
                        MenuResponse::None | MenuResponse::Closed => None,
                    }
                })
                .map(MenuResponse::Action)
                .unwrap_or(MenuResponse::None)
        };
        if let MenuResponse::Action(action) = response {
            if let Some(idx) = self.open_index {
                self.menus[idx].items = self.popup.items.clone();
            }
            (self.on_action)(&action);
            if !self.popup.is_open() {
                self.open_index = None;
            }
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.popup
            .paint(ctx, self.active_font(), self.font_size, current_viewport());
    }
}

#[cfg(test)]
mod tests_1;
#[cfg(test)]
mod tests_2;
