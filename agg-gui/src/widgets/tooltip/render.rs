//! Lightweight tooltip rendering: the fire-and-forget text-panel path.
//!
//! Non-interactive tooltips (the overwhelming majority of `Tooltip` uses)
//! submit a [`TooltipRequest`] during their `paint_overlay` pass. The App
//! drains this thread-local queue once the whole widget tree has painted
//! ([`paint_global_tooltips`]), so the panels float above scroll clips and
//! window content clips. This module owns that queue and its text painter.
//!
//! Interactive tooltips (see [`super::interactive`]) do **not** use this
//! queue for their own surface — they paint a real child widget tree in
//! `paint_global_overlay`. They *do* rely on it for any nested lightweight
//! tooltip hosted inside their content, which is why
//! [`paint_global_tooltips`] is drained a second time after the global
//! overlay pass (see `keyboard_scroll::paint_lifted_tree`).

use std::cell::RefCell;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Size};
use crate::text::Font;

use crate::geometry::Rect;

use super::{
    TooltipLine, TooltipLineKind, POINTER_TOOLTIP_EXTRA_DROP, SCREEN_MARGIN, TOOLTIP_FONT_SIZE,
    TOOLTIP_GAP, TOOLTIP_PAD_X, TOOLTIP_PAD_Y,
};

/// Height of one tooltip text line at [`TOOLTIP_FONT_SIZE`].
pub(super) const TOOLTIP_LINE_H: f64 = TOOLTIP_FONT_SIZE * 1.45;

/// Panel size for `line_count` text lines whose widest measures `max_text_w`.
/// Shared by the queue painter and the central controller so both agree on the
/// panel geometry (the controller measures with [`crate::text::measure_advance`],
/// the painter with `ctx.measure_text`, but the padding/line math is identical).
pub(super) fn panel_size(max_text_w: f64, line_count: usize) -> crate::geometry::Size {
    let panel_w = (max_text_w + TOOLTIP_PAD_X * 2.0).max(64.0);
    let panel_h = line_count as f64 * TOOLTIP_LINE_H + TOOLTIP_PAD_Y * 2.0;
    crate::geometry::Size::new(panel_w, panel_h)
}

/// Edge-aware placement for a tooltip panel of `panel` size anchored at
/// `anchor`: prefer below (and, for pointer tips, below-right of the cursor),
/// flip above when the bottom lacks room, then clamp fully inside the viewport
/// safe area. Pure so both the queue painter and the controller share one
/// clamp policy — and so the controller can expose the resulting rect to tests.
pub(super) fn place_panel(
    anchor: Point,
    panel: crate::geometry::Size,
    viewport: Size,
    at_pointer: bool,
) -> Rect {
    let panel_w = panel.width;
    let panel_h = panel.height;
    let mut panel_x = if at_pointer {
        anchor.x
    } else {
        anchor.x - panel_w * 0.5
    };
    let mut panel_y = anchor.y - panel_h - TOOLTIP_GAP;
    if at_pointer {
        panel_y -= POINTER_TOOLTIP_EXTRA_DROP;
    }
    if panel_x + panel_w > viewport.width - SCREEN_MARGIN {
        panel_x = viewport.width - panel_w - SCREEN_MARGIN;
    }
    if panel_y < SCREEN_MARGIN {
        // Not enough room below: fall back above the cursor / widget.
        panel_y = anchor.y + TOOLTIP_GAP;
    }
    panel_x = panel_x.clamp(
        SCREEN_MARGIN,
        (viewport.width - panel_w - SCREEN_MARGIN).max(SCREEN_MARGIN),
    );
    panel_y = panel_y.clamp(
        SCREEN_MARGIN,
        (viewport.height - panel_h - SCREEN_MARGIN).max(SCREEN_MARGIN),
    );
    Rect::new(panel_x, panel_y, panel_w, panel_h)
}

pub(super) struct TooltipRequest {
    pub font: Arc<Font>,
    pub lines: Vec<TooltipLine>,
    pub anchor: Point,
    pub at_pointer: bool,
}

thread_local! {
    static TOOLTIP_QUEUE: RefCell<Vec<TooltipRequest>> = const { RefCell::new(Vec::new()) };
    /// Viewport published at the start of each paint so interactive
    /// tooltips (which paint in `paint_global_overlay`, before the queue
    /// drain) can clamp their panels to the screen edge.
    static TOOLTIP_VIEWPORT: RefCell<Size> = const { RefCell::new(Size { width: 0.0, height: 0.0 }) };
}

pub(super) fn submit_tooltip(request: TooltipRequest) {
    TOOLTIP_QUEUE.with(|q| q.borrow_mut().push(request));
}

/// Font of the most recently queued tip, for tests that pin which face a tip
/// paints with (e.g. asserting the central controller re-reads the live system
/// font every paint). Pointer identity distinguishes two same-bytes fonts.
#[cfg(test)]
pub(super) fn last_submitted_font() -> Option<Arc<Font>> {
    TOOLTIP_QUEUE.with(|q| q.borrow().last().map(|r| Arc::clone(&r.font)))
}

/// Last viewport published by [`begin_tooltip_frame`]. Interactive
/// tooltips read this to keep their surface on-screen.
pub(super) fn current_tooltip_viewport() -> Size {
    TOOLTIP_VIEWPORT.with(|v| *v.borrow())
}

pub(crate) fn begin_tooltip_frame(viewport: Size) {
    TOOLTIP_QUEUE.with(|q| q.borrow_mut().clear());
    TOOLTIP_VIEWPORT.with(|v| *v.borrow_mut() = viewport);
}

pub(crate) fn paint_global_tooltips(ctx: &mut dyn DrawCtx, viewport: Size) {
    let requests = TOOLTIP_QUEUE.with(|q| q.borrow_mut().drain(..).collect::<Vec<_>>());
    for request in requests {
        paint_request(ctx, viewport, request);
    }
}

fn paint_request(ctx: &mut dyn DrawCtx, viewport: Size, request: TooltipRequest) {
    if request.lines.is_empty() {
        return;
    }

    let v = ctx.visuals();
    ctx.set_font(Arc::clone(&request.font));
    ctx.set_font_size(TOOLTIP_FONT_SIZE);

    let line_h = TOOLTIP_LINE_H;
    let mut max_w = 0.0_f64;
    for line in &request.lines {
        if let Some(m) = ctx.measure_text(&line.text) {
            max_w = max_w.max(m.width);
        }
    }

    let panel = place_panel(
        request.anchor,
        panel_size(max_w, request.lines.len()),
        viewport,
        request.at_pointer,
    );
    let (panel_x, panel_y, panel_w, panel_h) = (panel.x, panel.y, panel.width, panel.height);

    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.20));
    ctx.begin_path();
    ctx.rounded_rect(panel_x + 1.0, panel_y - 1.0, panel_w, panel_h, 5.0);
    ctx.fill();

    ctx.set_fill_color(v.window_fill);
    ctx.begin_path();
    ctx.rounded_rect(panel_x, panel_y, panel_w, panel_h, 5.0);
    ctx.fill();

    ctx.set_stroke_color(v.widget_stroke);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(panel_x, panel_y, panel_w, panel_h, 5.0);
    ctx.stroke();

    for (i, line) in request.lines.iter().enumerate() {
        let y = panel_y + panel_h - TOOLTIP_PAD_Y - (i as f64 + 1.0) * line_h + 2.0;
        match line.kind {
            TooltipLineKind::Text => {
                ctx.set_fill_color(v.text_color);
                ctx.fill_text(&line.text, panel_x + TOOLTIP_PAD_X, y);
            }
            TooltipLineKind::Code => {
                if let Some(m) = ctx.measure_text(&line.text) {
                    ctx.set_fill_color(v.track_bg);
                    ctx.begin_path();
                    ctx.rounded_rect(
                        panel_x + TOOLTIP_PAD_X - 3.0,
                        y - 3.0,
                        m.width + 6.0,
                        line_h,
                        3.0,
                    );
                    ctx.fill();
                }
                ctx.set_fill_color(v.text_color);
                ctx.fill_text(&line.text, panel_x + TOOLTIP_PAD_X, y);
            }
            TooltipLineKind::Link => {
                ctx.set_fill_color(v.text_link);
                ctx.fill_text(&line.text, panel_x + TOOLTIP_PAD_X, y);
                if let Some(m) = ctx.measure_text(&line.text) {
                    ctx.set_stroke_color(v.text_link);
                    ctx.set_line_width(1.0);
                    ctx.begin_path();
                    ctx.move_to(panel_x + TOOLTIP_PAD_X, y - 2.0);
                    ctx.line_to(panel_x + TOOLTIP_PAD_X + m.width, y - 2.0);
                    ctx.stroke();
                }
            }
        }
    }
}
