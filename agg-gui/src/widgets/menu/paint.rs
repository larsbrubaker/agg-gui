//! Themed painting for popup menus and menu bars.
//!
//! The painter deliberately sets every fill and stroke it uses so menu colors
//! never inherit accidental state from the widget that opened the popup.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;
use crate::text::Font;

use super::geometry::{PopupLayout, SEP_H};
use super::model::{MenuEntry, MenuSelection};
use super::state::PopupMenuState;

#[derive(Clone)]
pub struct MenuStyle {
    pub radius: f64,
    pub shadow_offset: (f64, f64),
    pub shadow_alpha: f32,
    pub pad_x: f64,
    pub icon_x: f64,
    pub label_x: f64,
    pub shortcut_right: f64,
}

impl Default for MenuStyle {
    fn default() -> Self {
        Self {
            radius: 5.0,
            shadow_offset: (5.0, -5.0),
            shadow_alpha: 0.22,
            pad_x: 8.0,
            icon_x: 14.0,
            label_x: 32.0,
            shortcut_right: 28.0,
        }
    }
}

pub fn paint_popup_stack(
    ctx: &mut dyn DrawCtx,
    font: Arc<Font>,
    font_size: f64,
    items: &[MenuEntry],
    state: &PopupMenuState,
    layouts: &[PopupLayout],
    style: &MenuStyle,
) {
    ctx.set_font(font);
    ctx.set_font_size(font_size);
    for layout in layouts {
        paint_panel(ctx, layout.rect, style);
        for (entry, row) in items_for_layout(items, &layout.path_prefix)
            .iter()
            .zip(&layout.rows)
        {
            match entry {
                MenuEntry::Separator => paint_separator(ctx, row.rect),
                MenuEntry::Item(item) => {
                    let mut path = layout.path_prefix.clone();
                    path.push(row.item_index.unwrap_or_default());
                    let hovered = state.hover_path.as_ref() == Some(&path);
                    let open = state.open_path.starts_with(&path);
                    paint_item_row(ctx, row.rect, item, hovered, open, style);
                }
            }
        }
    }
}

pub fn paint_menu_bar_button(
    ctx: &mut dyn DrawCtx,
    rect: Rect,
    label: &str,
    open: bool,
    hovered: bool,
) {
    let v = ctx.visuals();
    // Subtle desktop hover: a translucent accent tint under the label, not
    // the full accent.  Translucent because the underlying `top_bar_bg` is
    // already a very light gray (≈0.88 in the light theme); a fully-opaque
    // `widget_bg_hovered` panel reads as nothing — `widget_bg_hovered` is
    // only ~0.04 brighter than the bar.  Translucent accent stays visible
    // on either theme while reserving the FULL accent for the OPENED state.
    if open || hovered {
        let bg = if open {
            v.accent
        } else {
            v.accent.with_alpha(0.18)
        };
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(
            rect.x + 1.0,
            rect.y + 2.0,
            rect.width - 2.0,
            rect.height - 4.0,
            4.0,
        );
        ctx.fill();
    }
    ctx.set_fill_color(if open { Color::white() } else { v.text_color });
    ctx.fill_text(label, rect.x + 9.0, rect.y + 7.0);
}

fn paint_panel(ctx: &mut dyn DrawCtx, rect: Rect, style: &MenuStyle) {
    let v = ctx.visuals();
    ctx.set_fill_color(Color::black().with_alpha(style.shadow_alpha));
    ctx.begin_path();
    ctx.rounded_rect(
        rect.x + style.shadow_offset.0,
        rect.y + style.shadow_offset.1,
        rect.width,
        rect.height,
        style.radius,
    );
    ctx.fill();

    ctx.set_fill_color(v.panel_fill);
    ctx.begin_path();
    ctx.rounded_rect(rect.x, rect.y, rect.width, rect.height, style.radius);
    ctx.fill();
    ctx.set_stroke_color(v.widget_stroke);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(rect.x, rect.y, rect.width, rect.height, style.radius);
    ctx.stroke();
}

fn paint_separator(ctx: &mut dyn DrawCtx, rect: Rect) {
    let v = ctx.visuals();
    ctx.set_stroke_color(v.widget_stroke.with_alpha(0.55));
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(rect.x + 8.0, rect.y + SEP_H * 0.5);
    ctx.line_to(rect.x + rect.width - 8.0, rect.y + SEP_H * 0.5);
    ctx.stroke();
}

fn paint_item_row(
    ctx: &mut dyn DrawCtx,
    rect: Rect,
    item: &super::model::MenuItem,
    hovered: bool,
    open: bool,
    style: &MenuStyle,
) {
    let v = ctx.visuals();
    let hovered = hovered && item.enabled;
    let open = open && item.enabled;
    // Same subtle/strong split as `paint_menu_bar_button`: hover hints
    // with a translucent accent tint; open commits with full accent.
    if hovered || open {
        let bg = if open {
            v.accent
        } else {
            v.accent.with_alpha(0.18)
        };
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(
            rect.x + 3.0,
            rect.y + 2.0,
            rect.width - 6.0,
            rect.height - 4.0,
            3.0,
        );
        ctx.fill();
    }

    let text_color = if !item.enabled {
        v.text_color.with_alpha(0.45)
    } else if open {
        Color::white()
    } else {
        v.text_color
    };
    ctx.set_fill_color(text_color);
    if let Some(icon) = item.icon {
        let icon = icon.to_string();
        ctx.fill_text(&icon, rect.x + style.icon_x, rect.y + 7.0);
    } else {
        match item.selection {
            MenuSelection::Check { selected: true } => {
                ctx.fill_text("\u{f00c}", rect.x + style.icon_x, rect.y + 7.0);
            }
            MenuSelection::Radio { selected: true } => {
                ctx.fill_text("\u{f111}", rect.x + style.icon_x, rect.y + 7.0);
            }
            MenuSelection::None
            | MenuSelection::Check { selected: false }
            | MenuSelection::Radio { selected: false } => {}
        }
    }
    ctx.fill_text(&item.label, rect.x + style.label_x, rect.y + 7.0);
    if let Some(shortcut) = &item.shortcut {
        let width = ctx
            .measure_text(shortcut)
            .map(|metrics| metrics.width)
            .unwrap_or(0.0);
        ctx.fill_text(
            shortcut,
            rect.x + rect.width - style.shortcut_right - width,
            rect.y + 7.0,
        );
    }
    if item.has_submenu() {
        ctx.fill_text("\u{f105}", rect.x + rect.width - 18.0, rect.y + 7.0);
    }
}

fn items_for_layout<'a>(items: &'a [MenuEntry], path: &[usize]) -> &'a [MenuEntry] {
    let mut current = items;
    for &idx in path {
        let Some(MenuEntry::Item(item)) = current.get(idx) else {
            return current;
        };
        current = &item.submenu;
    }
    current
}
