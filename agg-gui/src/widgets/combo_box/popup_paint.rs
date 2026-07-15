//! Global ComboBox popup paint pass.
//!
//! Each `ComboBox::paint_global_overlay` enqueues a `ComboPopupRequest` for
//! its open dropdown; `paint_global_combo_popups` is invoked from `App::paint`
//! after the regular tree walk and after `paint_global_overlays`, so popups
//! always paint on top of the rest of the UI (including modals).
//!
//! Coordinates in the request are LOGICAL root-space — the drain pass runs
//! while the outer device-scale CTM is still active, so logical x/y multiply
//! up to physical pixels naturally and we avoid double-scaling on HiDPI.

use super::{
    submit_combo_popup_internal, ComboPopupRequest, COMBO_POPUP_QUEUE, CORNER_R,
    CURRENT_COMBO_VIEWPORT, ITEM_H, PAD_X, SCROLLBAR_W,
};
use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::Size;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::scrollbar::paint_prepared_scrollbar;

pub(crate) fn submit_combo_popup(request: ComboPopupRequest) {
    submit_combo_popup_internal(request);
}

pub(crate) fn current_combo_viewport() -> Option<Size> {
    CURRENT_COMBO_VIEWPORT.with(|v| v.get())
}

pub(crate) fn begin_combo_popup_frame(viewport: Size) {
    CURRENT_COMBO_VIEWPORT.with(|v| v.set(Some(viewport)));
    COMBO_POPUP_QUEUE.with(|q| q.borrow_mut().clear());
}

pub(crate) fn paint_global_combo_popups(ctx: &mut dyn DrawCtx) {
    let requests = COMBO_POPUP_QUEUE.with(|q| q.borrow_mut().drain(..).collect::<Vec<_>>());
    if requests.is_empty() {
        return;
    }

    ctx.save();
    ctx.reset_clip();
    for request in requests {
        paint_combo_popup(ctx, request);
    }
    ctx.restore();
}

fn paint_combo_popup(ctx: &mut dyn DrawCtx, request: ComboPopupRequest) {
    let v = ctx.visuals();
    let popup_y = if request.opens_up {
        request.y + super::CLOSED_H
    } else {
        request.y - request.popup_h
    };

    // Opaque backing first. Some widget fills are intentionally subtle; the
    // popup itself must always obscure the content underneath.
    ctx.set_fill_color(v.window_fill);
    ctx.begin_path();
    ctx.rounded_rect(request.x, popup_y, request.width, request.popup_h, CORNER_R);
    ctx.fill();

    ctx.set_fill_color(v.widget_bg);
    ctx.begin_path();
    ctx.rounded_rect(request.x, popup_y, request.width, request.popup_h, CORNER_R);
    ctx.fill();

    let has_scroll = request.item_count > request.visible_count;
    let text_w = if has_scroll {
        (request.width - SCROLLBAR_W - 4.0).max(0.0)
    } else {
        request.width
    };

    // Borrow the shared item-label vec for the duration of the popup
    // paint.  Painting through `paint_subtree(label, ctx)` keeps each
    // Label's backbuffer cache hot — text is rasterised once and the
    // bitmap is blitted on subsequent frames, instead of the prior
    // raw `ctx.fill_text` per item per frame.
    let mut labels = request.item_labels.borrow_mut();
    for row in 0..request.visible_count {
        let idx = request.first_item + row;
        if idx >= labels.len() {
            break;
        }
        let item_y = popup_y + request.popup_h - (row as f64 + 1.0) * ITEM_H;
        let is_selected = idx == request.selected;
        let is_hovered = request.hovered_item == Some(idx);
        if is_selected || is_hovered {
            let bg = if is_selected {
                v.accent
            } else {
                v.widget_bg_hovered
            };
            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.rounded_rect(
                request.x + 2.0,
                item_y + 1.0,
                text_w - 4.0,
                ITEM_H - 2.0,
                3.0,
            );
            ctx.fill();
        }

        let label = &mut labels[idx];
        label.set_color(if is_selected {
            Color::white()
        } else {
            v.text_color
        });
        // Translate the ctx so the Label paints at its desired global
        // popup position.  Label's own paint routes through its
        // backbuffer cache via `paint_subtree`, so glyph rasterisation
        // is shared across frames and per-item-font preview combos
        // stay snappy.
        let lh = label.bounds().height;
        let ly = item_y + (ITEM_H - lh) * 0.5;
        ctx.save();
        ctx.translate(request.x + PAD_X, ly);
        paint_subtree(label, ctx);
        ctx.restore();
    }
    drop(labels);

    if let Some(scrollbar) = request.scrollbar {
        paint_prepared_scrollbar(ctx, scrollbar);
    }

    ctx.set_stroke_color(v.widget_stroke);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(request.x, popup_y, request.width, request.popup_h, CORNER_R);
    ctx.stroke();

    paint_item_hover_tooltip(ctx, &request, popup_y);
}

/// Paint the per-item hover tooltip (from `ComboBox::with_item_tooltips`)
/// beside the hovered row.  Painted immediately (no hover delay) because the
/// dropdown is already an explicit, transient interaction — unlike the
/// wander-prone main canvas the `Tooltip` widget's 500 ms delay protects.
fn paint_item_hover_tooltip(ctx: &mut dyn DrawCtx, request: &ComboPopupRequest, popup_y: f64) {
    let Some((text, font)) = &request.hover_tooltip else {
        return;
    };
    let Some(hovered) = request.hovered_item else {
        return;
    };
    // Only rows currently scrolled into view can anchor a tooltip.
    if hovered < request.first_item || hovered >= request.first_item + request.visible_count {
        return;
    }
    let v = ctx.visuals();
    const FONT_SIZE: f64 = 12.0;
    const PAD: f64 = 7.0;
    const GAP: f64 = 6.0;
    ctx.set_font(std::sync::Arc::clone(font));
    ctx.set_font_size(FONT_SIZE);
    let text_w = ctx.measure_text(text).map(|m| m.width).unwrap_or(0.0);
    let panel_w = text_w + PAD * 2.0;
    let panel_h = FONT_SIZE * 1.5 + PAD * 2.0 - 4.0;

    let row = hovered - request.first_item;
    let item_y = popup_y + request.popup_h - (row as f64 + 1.0) * ITEM_H;
    let mut px = request.x + request.width + GAP;
    let mut py = item_y + (ITEM_H - panel_h) * 0.5;
    // Keep on-screen: fall back to the popup's left side, clamp vertically.
    if let Some(viewport) = current_combo_viewport() {
        if px + panel_w > viewport.width - 4.0 {
            px = (request.x - GAP - panel_w).max(4.0);
        }
        py = py.clamp(4.0, (viewport.height - panel_h - 4.0).max(4.0));
    }

    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.20));
    ctx.begin_path();
    ctx.rounded_rect(px + 1.0, py - 1.0, panel_w, panel_h, 4.0);
    ctx.fill();
    ctx.set_fill_color(v.window_fill);
    ctx.begin_path();
    ctx.rounded_rect(px, py, panel_w, panel_h, 4.0);
    ctx.fill();
    ctx.set_stroke_color(v.widget_stroke);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(px, py, panel_w, panel_h, 4.0);
    ctx.stroke();
    ctx.set_fill_color(v.text_color);
    ctx.fill_text(text, px + PAD, py + PAD - 1.0);
}
