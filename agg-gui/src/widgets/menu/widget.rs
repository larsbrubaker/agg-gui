//! Widget adapters for reusable menus.
//!
//! `ContextMenu` is a small controller that other widgets can embed, while
//! `MenuBar` is a visible widget for top-level menus.

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::{current_viewport, Widget};

use super::geometry::{contains, item_at_path, BAR_H};
use super::model::MenuEntry;
use super::paint::{paint_menu_bar_button, paint_popup_stack, MenuStyle};
use super::state::{MenuAnchorKind, MenuResponse, PopupMenuState};

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
        }
    }

    pub fn with_font_size(mut self, font_size: f64) -> Self {
        self.font_size = font_size;
        self
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
        if self.hover_index != hover {
            self.hover_index = hover;
            crate::animation::request_draw_without_invalidation();
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
        Size::new(available.width, BAR_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        let v = ctx.visuals();
        ctx.set_fill_color(v.top_bar_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, BAR_H);
        ctx.fill();
        for (idx, menu) in self.menus.iter().enumerate() {
            paint_menu_bar_button(
                ctx,
                menu.rect,
                &menu.label,
                self.open_index == Some(idx),
                self.hover_index == Some(idx),
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
            if self.popup.is_open() {
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
        self.popup.paint(
            ctx,
            Arc::clone(&self.font),
            self.font_size,
            current_viewport(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Modifiers, MouseButton};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn test_font() -> Arc<Font> {
        const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");
        Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
    }

    #[test]
    fn moving_across_top_menus_switches_open_popup() {
        let mut bar = MenuBar::new(
            test_font(),
            vec![
                TopMenu::new(
                    "File",
                    vec![super::super::model::MenuItem::action("New", "file.new").into()],
                ),
                TopMenu::new(
                    "Edit",
                    vec![super::super::model::MenuItem::action("Copy", "edit.copy").into()],
                ),
            ],
            |_| {},
        );
        bar.layout(Size::new(300.0, BAR_H));

        assert_eq!(
            bar.on_event(&Event::MouseDown {
                pos: Point::new(8.0, 8.0),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            }),
            EventResult::Consumed
        );
        assert_eq!(bar.open_index, Some(0));

        assert_eq!(
            bar.on_event(&Event::MouseMove {
                pos: Point::new(60.0, 8.0),
            }),
            EventResult::Consumed
        );
        assert_eq!(bar.open_index, Some(1));
        let Some(super::super::model::MenuEntry::Item(item)) = bar.popup.items.first() else {
            panic!("popup should contain Edit items");
        };
        assert_eq!(item.action.as_deref(), Some("edit.copy"));
    }

    #[test]
    fn top_level_menu_tracks_hover() {
        let mut bar = MenuBar::new(
            test_font(),
            vec![TopMenu::new(
                "File",
                vec![super::super::model::MenuItem::action("New", "file.new").into()],
            )],
            |_| {},
        );
        bar.layout(Size::new(300.0, BAR_H));

        assert_eq!(
            bar.on_event(&Event::MouseMove {
                pos: Point::new(8.0, 8.0),
            }),
            EventResult::Ignored
        );
        assert_eq!(bar.hover_index, Some(0));
    }

    #[test]
    fn mouse_down_drag_release_activates_popup_item() {
        let viewport = Size::new(300.0, 180.0);
        crate::widget::set_current_viewport(viewport);
        let actions = Rc::new(RefCell::new(Vec::new()));
        let actions_for_cb = Rc::clone(&actions);
        let mut bar = MenuBar::new(
            test_font(),
            vec![TopMenu::new(
                "File",
                vec![super::super::model::MenuItem::action("New", "file.new").into()],
            )],
            move |action| actions_for_cb.borrow_mut().push(action.to_string()),
        );
        bar.layout(Size::new(300.0, BAR_H));

        assert_eq!(
            bar.on_event(&Event::MouseDown {
                pos: Point::new(8.0, 8.0),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            }),
            EventResult::Consumed
        );
        let row = bar.popup.state.layouts(&bar.popup.items, viewport)[0].rows[0].rect;
        let item_pos = Point::new(row.x + 12.0, row.y + 12.0);

        assert_eq!(
            bar.on_event(&Event::MouseMove { pos: item_pos }),
            EventResult::Consumed
        );
        assert_eq!(
            bar.on_event(&Event::MouseUp {
                pos: item_pos,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            }),
            EventResult::Consumed
        );

        assert_eq!(actions.borrow().as_slice(), ["file.new"]);
        assert!(!bar.popup.is_open());
    }

    #[test]
    fn simple_mouse_click_opens_menu_without_release_activation() {
        let viewport = Size::new(300.0, 180.0);
        crate::widget::set_current_viewport(viewport);
        let mut bar = MenuBar::new(
            test_font(),
            vec![TopMenu::new(
                "File",
                vec![super::super::model::MenuItem::action("New", "file.new").into()],
            )],
            |_| {},
        );
        bar.layout(Size::new(300.0, BAR_H));

        assert_eq!(
            bar.on_event(&Event::MouseDown {
                pos: Point::new(8.0, 8.0),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            }),
            EventResult::Consumed
        );
        assert_eq!(
            bar.on_event(&Event::MouseUp {
                pos: Point::new(8.0, 8.0),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            }),
            EventResult::Consumed
        );

        assert!(bar.popup.is_open());
        assert_eq!(bar.open_index, Some(0));
    }

    #[test]
    fn unconsumed_shortcut_fires_top_menu_action() {
        let actions = Rc::new(RefCell::new(Vec::new()));
        let actions_for_cb = Rc::clone(&actions);
        let mut bar = MenuBar::new(
            test_font(),
            vec![TopMenu::new(
                "File",
                vec![super::super::model::MenuItem::action("New", "file.new")
                    .shortcut("Ctrl+N")
                    .into()],
            )],
            move |action| actions_for_cb.borrow_mut().push(action.to_string()),
        );

        assert_eq!(
            bar.on_unconsumed_key(
                &Key::Char('n'),
                Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
            ),
            EventResult::Consumed
        );

        assert_eq!(actions.borrow().as_slice(), ["file.new"]);
    }

    #[test]
    fn arrow_keys_switch_open_top_menus() {
        let mut bar = MenuBar::new(
            test_font(),
            vec![
                TopMenu::new(
                    "File",
                    vec![super::super::model::MenuItem::action("New", "file.new").into()],
                ),
                TopMenu::new(
                    "Edit",
                    vec![super::super::model::MenuItem::action("Copy", "edit.copy").into()],
                ),
            ],
            |_| {},
        );
        bar.layout(Size::new(300.0, BAR_H));
        bar.open_menu(0);

        assert_eq!(
            bar.on_event(&Event::KeyDown {
                key: Key::ArrowRight,
                modifiers: Modifiers::default(),
            }),
            EventResult::Consumed
        );
        assert_eq!(bar.open_index, Some(1));

        assert_eq!(
            bar.on_event(&Event::KeyDown {
                key: Key::ArrowLeft,
                modifiers: Modifiers::default(),
            }),
            EventResult::Consumed
        );
        assert_eq!(bar.open_index, Some(0));
    }
}
