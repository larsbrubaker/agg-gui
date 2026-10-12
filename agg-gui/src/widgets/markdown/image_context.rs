//! Right-click menus for `MarkdownView`: image actions on an image, and
//! elsewhere MarkdigAgg's read-only text menu (`MarkdownWidget.ShowRightClickMenu`:
//! Cut and Paste disabled, Copy enabled with a selection, Select All enabled
//! when there is text).
//!
//! This module adapts the actions onto the shared menu infrastructure so
//! Markdown context menus match the rest of the UI.

use crate::clipboard;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, Modifiers, MouseButton};
use crate::geometry::Point;
use crate::widget::current_viewport;
use crate::widgets::menu::{MenuEntry, MenuItem, MenuResponse, PopupMenu};

use super::{ImageState, MarkdownView};

#[derive(Clone)]
pub(super) struct MarkdownContextMenuState {
    /// The image right-clicked, `None` for the text menu.
    image: Option<ImageContextTarget>,
    menu: PopupMenu,
}

#[derive(Clone)]
struct ImageContextTarget {
    url: String,
    alt: String,
    cache_idx: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ImageContextAction {
    CopyImage,
    CopyImageUrl,
    OpenImage,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

impl ImageContextAction {
    fn label(self) -> &'static str {
        match self {
            ImageContextAction::CopyImage => "Copy Image",
            ImageContextAction::CopyImageUrl => "Copy Image URL",
            ImageContextAction::OpenImage => "Open Image",
            ImageContextAction::Cut => "Cut",
            ImageContextAction::Copy => "Copy",
            ImageContextAction::Paste => "Paste",
            ImageContextAction::SelectAll => "Select All",
        }
    }

    fn id(self) -> &'static str {
        match self {
            ImageContextAction::CopyImage => "copy-image",
            ImageContextAction::CopyImageUrl => "copy-image-url",
            ImageContextAction::OpenImage => "open-image",
            ImageContextAction::Cut => "cut",
            ImageContextAction::Copy => "copy",
            ImageContextAction::Paste => "paste",
            ImageContextAction::SelectAll => "select-all",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        match id {
            "copy-image" => Some(Self::CopyImage),
            "copy-image-url" => Some(Self::CopyImageUrl),
            "open-image" => Some(Self::OpenImage),
            "cut" => Some(Self::Cut),
            "copy" => Some(Self::Copy),
            "paste" => Some(Self::Paste),
            "select-all" => Some(Self::SelectAll),
            _ => None,
        }
    }
}

impl MarkdownView {
    /// The open right-click menu's rows named `name`, as named targets in
    /// the view's local space (the menu opens at a view-local point), for
    /// GUI automation.
    pub(super) fn context_menu_named_targets(&self, name: &str, out: &mut Vec<crate::NamedTarget>) {
        if let Some(state) = self.context_menu.as_ref() {
            state.menu.named_row_targets(name, current_viewport(), out);
        }
    }

    /// Open the image menu on an image, else the text menu.
    pub(super) fn open_context_menu(&mut self, pos: Point) -> bool {
        if self.open_image_context_menu(pos) {
            return true;
        }
        let action = |action: ImageContextAction, enabled: bool| -> MenuEntry {
            let item = MenuItem::action(action.label(), action.id());
            if enabled { item } else { item.disabled() }.into()
        };
        let items = vec![
            action(ImageContextAction::Cut, false),
            action(ImageContextAction::Copy, self.has_selection()),
            action(ImageContextAction::Paste, false),
            MenuEntry::Separator,
            action(
                ImageContextAction::SelectAll,
                !self.selectable_text.is_empty(),
            ),
        ];
        let mut menu = PopupMenu::new(items);
        menu.open_at(pos);
        self.context_menu = Some(MarkdownContextMenuState { image: None, menu });
        true
    }

    fn open_image_context_menu(&mut self, pos: Point) -> bool {
        let Some((url, alt, cache_idx)) = self.hit_image(pos) else {
            self.context_menu = None;
            return false;
        };
        let mut actions = vec![
            ImageContextAction::CopyImage,
            ImageContextAction::CopyImageUrl,
        ];
        if self.on_image_open.is_some() {
            actions.push(ImageContextAction::OpenImage);
        }
        let items = actions
            .iter()
            .map(|action| MenuItem::action(action.label(), action.id()).into())
            .collect::<Vec<MenuEntry>>();
        let mut menu = PopupMenu::new(items);
        menu.open_at(pos);
        self.context_menu = Some(MarkdownContextMenuState {
            image: Some(ImageContextTarget {
                url,
                alt,
                cache_idx,
            }),
            menu,
        });
        true
    }

    pub(super) fn update_context_menu_hover(&mut self, pos: Point) -> bool {
        let Some(menu) = self.context_menu.as_mut() else {
            return false;
        };
        let event = Event::MouseMove { pos };
        let (result, _) = menu.menu.handle_event(&event, current_viewport());
        result.is_consumed()
    }

    pub(super) fn context_menu_contains(&self, _pos: Point) -> bool {
        self.context_menu
            .as_ref()
            .map(|menu| menu.menu.is_open())
            .unwrap_or(false)
    }

    pub(super) fn handle_context_menu_mouse_down(&mut self, pos: Point) -> bool {
        let Some(menu_state) = self.context_menu.as_mut() else {
            return false;
        };
        let event = Event::MouseDown {
            pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        };
        let (result, response) = menu_state.menu.handle_event(&event, current_viewport());
        match response {
            MenuResponse::Action(action_id) => {
                let image = menu_state.image.clone();
                self.context_menu = None;
                self.suppress_next_left_mouse_up = true;
                if let Some(action) = ImageContextAction::from_id(&action_id) {
                    self.run_menu_action(action, image.as_ref());
                }
            }
            MenuResponse::Closed => {
                self.context_menu = None;
                self.suppress_next_left_mouse_up = true;
            }
            MenuResponse::None => {}
        }
        result.is_consumed()
    }

    fn run_menu_action(&mut self, action: ImageContextAction, image: Option<&ImageContextTarget>) {
        match action {
            ImageContextAction::Copy => {
                self.try_copy_selection_to_clipboard();
                return;
            }
            ImageContextAction::SelectAll => {
                self.select_all_text();
                crate::animation::request_draw();
                return;
            }
            ImageContextAction::Cut | ImageContextAction::Paste => return,
            _ => {}
        }
        let Some(image) = image else {
            return;
        };
        match action {
            ImageContextAction::CopyImage => {
                if !self.copy_image_pixels(image.cache_idx) {
                    if image.alt.is_empty() {
                        clipboard::set_text(&image.url);
                    } else {
                        clipboard::set_text(&format!("![{}]({})", image.alt, image.url));
                    }
                }
            }
            ImageContextAction::CopyImageUrl => clipboard::set_text(&image.url),
            ImageContextAction::OpenImage => {
                if let Some(cb) = self.on_image_open.as_mut() {
                    cb(&image.url);
                }
            }
            _ => {}
        }
    }

    /// Whether the open context menu's item `label` is enabled (`None` when
    /// no menu is open or it has no such item).
    pub fn context_menu_item_enabled(&self, label: &str) -> Option<bool> {
        let menu = &self.context_menu.as_ref()?.menu;
        menu.items.iter().find_map(|entry| match entry {
            MenuEntry::Item(item) if item.label == label => Some(item.enabled),
            _ => None,
        })
    }

    /// The centre of the open context menu's row `label`, where a click
    /// chooses it (`None` when no menu is open or it has no such item).
    pub fn context_menu_item_center(&self, label: &str) -> Option<Point> {
        let menu = &self.context_menu.as_ref()?.menu;
        let layouts = menu.state.layouts(&menu.items, current_viewport());
        let panel = layouts.first()?;
        panel.rows.iter().find_map(|row| {
            let idx = row.item_index?;
            match menu.items.get(idx) {
                Some(MenuEntry::Item(item)) if item.label == label => Some(Point::new(
                    row.rect.x + row.rect.width * 0.5,
                    row.rect.y + row.rect.height * 0.5,
                )),
                _ => None,
            }
        })
    }

    fn copy_image_pixels(&self, cache_idx: usize) -> bool {
        let Some(entry) = self.image_cache.get(cache_idx) else {
            return false;
        };
        let Ok(state) = entry.state.lock() else {
            return false;
        };
        if let ImageState::Ready { image, .. } = &*state {
            clipboard::set_image_rgba(image.data.as_slice(), image.width, image.height)
        } else {
            false
        }
    }

    pub(super) fn paint_context_menu(&mut self, ctx: &mut dyn DrawCtx) {
        let font = std::sync::Arc::clone(&self.active_font());
        let font_size = self.font_size;
        let Some(menu_state) = &mut self.context_menu else {
            return;
        };
        menu_state
            .menu
            .paint(ctx, font, font_size, current_viewport());
    }
}
