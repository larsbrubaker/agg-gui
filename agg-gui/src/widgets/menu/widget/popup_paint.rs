//! Popup-panel painting extracted from [`super`] (`widget/mod.rs`) to keep
//! that file under the 800-line cap.
//!
//! `paint_popup_level` draws one open popup panel — hover / open
//! backgrounds, separators, inline icon / check / radio glyphs, submenu
//! chevrons — and delegates the row text to the shared [`PopupLabels`]
//! cache so every glyph flows through the framework's backbuffer + LCD
//! path.  It consumes the row rects produced by
//! [`super::super::geometry::stack_layout`], so it automatically paints at
//! whatever (possibly touch-grown) metrics that layout used.

use crate::draw_ctx::DrawCtx;

use super::super::geometry::PopupLayout;
use super::super::model::{MenuEntry, MenuSelection};
use super::super::paint::{
    paint_check_mark, paint_item_row_bg, paint_separator, paint_submenu_chevron,
    popup_row_text_color, MenuStyle,
};
use super::super::state::PopupMenuState;
use super::labels::PopupLabels;

/// Paint one popup panel: hover backgrounds, separators, icons,
/// check / radio marks, submenu chevrons, AND each row's text via the
/// shared `PopupLabels` cache.  Text colour is resolved per-row from
/// the item's enabled / open / hovered state so a hover flip retints
/// just one Label.
pub(super) fn paint_popup_level(
    ctx: &mut dyn DrawCtx,
    level_idx: usize,
    layout: &PopupLayout,
    items: &[MenuEntry],
    state: &PopupMenuState,
    style: &MenuStyle,
    labels: &mut PopupLabels,
) {
    let level_items = items_for_layout(items, &layout.path_prefix);
    for (row_idx, row_layout) in layout.rows.iter().enumerate() {
        let Some(item_idx) = row_layout.item_index else {
            // Separator row.
            paint_separator(ctx, row_layout.rect);
            continue;
        };
        let Some(MenuEntry::Item(item)) = level_items.get(item_idx) else {
            continue;
        };
        let mut path = layout.path_prefix.clone();
        path.push(item_idx);
        let hovered = state.hover_path.as_ref() == Some(&path);
        let open = state.open_path.starts_with(&path);

        // Hover / open backdrop sits underneath the row's content.
        paint_item_row_bg(ctx, row_layout.rect, hovered, open, item.enabled);

        // Inline glyphs (icon / check / radio) — single chars that
        // change infrequently; keeping them direct `fill_text` avoids
        // a Label per glyph at the cost of a single rasterise call.
        let inline_color = popup_row_text_color(ctx, item.enabled, open && item.enabled);
        ctx.set_fill_color(inline_color);
        if let Some(color) = item.swatch {
            // Colour-swatch row (e.g., an accent palette).  A small
            // rounded filled rect in the icon slot, anchored
            // vertically to the row centre.  The selected radio gets a
            // thin stroke around the swatch instead of the usual radio
            // glyph so the colour itself stays the dominant cue.
            let size = 12.0;
            let sx = row_layout.rect.x + style.icon_x - size * 0.5 + 4.0;
            let sy = row_layout.rect.y + (row_layout.rect.height - size) * 0.5;
            let fill = if item.enabled {
                color
            } else {
                // Wash out disabled swatches so they read as "not
                // pickable" without losing their identity.
                color.with_alpha(0.45)
            };
            ctx.set_fill_color(fill);
            ctx.begin_path();
            ctx.rounded_rect(sx, sy, size, size, 3.0);
            ctx.fill();
            if matches!(item.selection, MenuSelection::Radio { selected: true }) {
                ctx.set_stroke_color(inline_color);
                ctx.set_line_width(1.5);
                ctx.begin_path();
                ctx.rounded_rect(sx - 2.0, sy - 2.0, size + 4.0, size + 4.0, 4.0);
                ctx.stroke();
            }
        } else if let Some(icon) = item.icon {
            let icon = icon.to_string();
            ctx.fill_text(
                &icon,
                row_layout.rect.x + style.icon_x,
                row_layout.rect.y + 7.0,
            );
        }
        // Selection indicator. Vector-drawn check mark shared by both
        // Check and Radio rows so the indicator looks identical
        // regardless of the host's icon font (or lack of one). When
        // the row already has a left-side marker (icon or swatch),
        // the check paints at the right edge so the marker's
        // identity stays visible; otherwise it occupies the icon
        // slot.
        let selected = matches!(
            item.selection,
            MenuSelection::Check { selected: true } | MenuSelection::Radio { selected: true }
        );
        if selected {
            let has_left_marker = item.swatch.is_some() || item.icon.is_some();
            let cx = if has_left_marker {
                let right_offset = if item.has_submenu() { 30.0 } else { 12.0 };
                row_layout.rect.x + row_layout.rect.width - right_offset
            } else {
                row_layout.rect.x + style.icon_x
            };
            let cy = row_layout.rect.y + row_layout.rect.height * 0.5;
            paint_check_mark(ctx, cx, cy, inline_color);
        }
        if item.has_submenu() {
            paint_submenu_chevron(ctx, row_layout.rect, inline_color);
        }

        // Row text (label + shortcut) via the Label cache so glyph
        // rendering routes through the backbuffer + LCD path.
        labels.paint_row_with_state(
            ctx,
            level_idx,
            row_idx,
            row_layout.rect,
            style.label_x,
            style.shortcut_right,
            item.enabled,
            open && item.enabled,
        );
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
