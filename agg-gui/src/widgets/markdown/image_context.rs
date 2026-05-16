//! Right-click image actions for `MarkdownView`.
//!
//! This module adapts image-specific actions onto the shared menu
//! infrastructure so Markdown context menus match the rest of the UI.

use crate::clipboard;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Modifiers, MouseButton};
use crate::geometry::Point;
use crate::widget::current_viewport;
use crate::widgets::menu::{MenuEntry, MenuItem, MenuResponse, PopupMenu};

use super::{ImageState, MarkdownView};

#[derive(Clone)]
pub(super) struct MarkdownContextMenuState {
    image: ImageContextTarget,
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
}

impl ImageContextAction {
    fn label(self) -> &'static str {
        match self {
            ImageContextAction::CopyImage => "Copy Image",
            ImageContextAction::CopyImageUrl => "Copy Image URL",
            ImageContextAction::OpenImage => "Open Image",
        }
    }

    fn id(self) -> &'static str {
        match self {
            ImageContextAction::CopyImage => "copy-image",
            ImageContextAction::CopyImageUrl => "copy-image-url",
            ImageContextAction::OpenImage => "open-image",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        match id {
            "copy-image" => Some(Self::CopyImage),
            "copy-image-url" => Some(Self::CopyImageUrl),
            "open-image" => Some(Self::OpenImage),
            _ => None,
        }
    }
}

impl MarkdownView {
    pub(super) fn open_image_context_menu(&mut self, pos: Point) -> bool {
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
            image: ImageContextTarget {
                url,
                alt,
                cache_idx,
            },
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
        result == EventResult::Consumed
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
                    self.run_image_action(action, &image);
                }
            }
            MenuResponse::Closed => {
                self.context_menu = None;
                self.suppress_next_left_mouse_up = true;
            }
            MenuResponse::None => {}
        }
        result == EventResult::Consumed
    }

    fn run_image_action(&mut self, action: ImageContextAction, image: &ImageContextTarget) {
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
        }
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
        menu_state.menu.paint(ctx, font, font_size, current_viewport());
    }
}
