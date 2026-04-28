//! Right-click image actions for `MarkdownView`.
//!
//! This module owns the small Markdown-local context menu used to copy image
//! pixels or URLs and to delegate image opening back to the application.

use crate::clipboard;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};

use super::{ImageState, MarkdownView};

#[derive(Clone)]
pub(super) struct MarkdownContextMenuState {
    pos: Point,
    image: ImageContextTarget,
    actions: Vec<ImageContextAction>,
}

#[derive(Clone)]
struct ImageContextTarget {
    url: String,
    alt: String,
    cache_idx: usize,
}

#[derive(Clone, Copy)]
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
        self.context_menu = Some(MarkdownContextMenuState {
            pos,
            image: ImageContextTarget {
                url,
                alt,
                cache_idx,
            },
            actions,
        });
        true
    }

    pub(super) fn handle_context_menu_mouse_down(&mut self, pos: Point) -> bool {
        let Some(menu) = self.context_menu.clone() else {
            return false;
        };
        if let Some(action) = menu.action_at(pos) {
            self.run_image_action(action, &menu.image);
            self.context_menu = None;
            crate::animation::request_draw();
            true
        } else if menu.bounds().contains(pos) {
            true
        } else {
            self.context_menu = None;
            crate::animation::request_draw();
            false
        }
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

    pub(super) fn paint_context_menu(&self, ctx: &mut dyn DrawCtx) {
        let Some(menu) = &self.context_menu else {
            return;
        };
        let bounds = menu.bounds();
        let v = ctx.visuals();
        ctx.set_fill_color(v.panel_fill);
        ctx.begin_path();
        ctx.rounded_rect(bounds.x, bounds.y, bounds.width, bounds.height, 4.0);
        ctx.fill();
        ctx.set_fill_color(v.widget_stroke);
        ctx.begin_path();
        ctx.rounded_rect(bounds.x, bounds.y, bounds.width, bounds.height, 4.0);
        ctx.stroke();

        ctx.set_font(std::sync::Arc::clone(&self.active_font()));
        ctx.set_font_size(self.font_size);
        for (idx, action) in menu.actions.iter().enumerate() {
            let row_y = bounds.y + bounds.height - (idx as f64 + 1.0) * MENU_ROW_H;
            ctx.set_fill_color(v.text_color);
            ctx.fill_text(action.label(), bounds.x + MENU_PAD_X, row_y + 8.0);
        }
    }
}

impl MarkdownContextMenuState {
    fn bounds(&self) -> Rect {
        Rect::new(
            self.pos.x,
            self.pos.y - self.actions.len() as f64 * MENU_ROW_H,
            MENU_W,
            self.actions.len() as f64 * MENU_ROW_H,
        )
    }

    fn action_at(&self, pos: Point) -> Option<ImageContextAction> {
        let bounds = self.bounds();
        if !bounds.contains(pos) {
            return None;
        }
        let from_top = ((bounds.y + bounds.height - pos.y) / MENU_ROW_H).floor() as usize;
        self.actions.get(from_top).copied()
    }
}

const MENU_W: f64 = 144.0;
const MENU_ROW_H: f64 = 24.0;
const MENU_PAD_X: f64 = 10.0;

trait RectContains {
    fn contains(self, point: Point) -> bool;
}

impl RectContains for Rect {
    fn contains(self, point: Point) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.width
            && point.y >= self.y
            && point.y <= self.y + self.height
    }
}
