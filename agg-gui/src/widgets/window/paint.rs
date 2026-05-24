// Painting helpers for `Window`.  Lifted out of `widget_impl.rs` so
// that file stays under the 800-line limit enforced by
// `tests/file_line_count.rs`.  Each helper takes `&mut Window`
// instead of `&mut self` so the trait `impl Widget for Window` can
// remain a thin dispatcher in `widget_impl.rs`.

use super::*;
use crate::widget::paint_subtree;

/// Body of `<Window as Widget>::paint`.
pub(super) fn paint_window(window: &mut Window, ctx: &mut dyn DrawCtx) {
    if !window.is_visible() {
        return;
    }

    let v = ctx.visuals();
    let w = window.bounds.width;
    // bounds.height == TITLE_H when collapsed (adjusted on toggle).
    let h = window.bounds.height;

    // Drop shadow + body fill go through the shared chrome helpers so
    // every "framed" widget (Window, NodeWidget, etc.) renders the same
    // halo + rounded body.
    let style = super::chrome::ChromeStyle::from_visuals(&v);
    super::chrome::paint_chrome_shadow(ctx, w, h, &style);

    window.foreground_layer_active.set(false);
    if ctx.supports_compositing_layers() {
        ctx.push_layer(w, h);
        window.foreground_layer_active.set(true);
    }

    // Window body. Expanded windows leave the top strip to `WindowTitleBar`
    // so the top corner alpha comes from one shape, not overlapping fills.
    super::chrome::paint_chrome_body(ctx, w, h, &style, window.collapsed);

    ctx.set_layer_rounded_clip(0.0, 0.0, w, h, CORNER_R);

    // Sync the title-bar sub-widget's display state for this frame
    // and paint it.  Positioning was done in `layout`; we just need
    // to hand it the per-frame interaction snapshot and dispatch
    // through `paint_subtree` so the ancestor-chain stack gets the
    // WindowTitleBar entry (background_color = window_title_fill).
    {
        let mut st = window.title_state.borrow_mut();
        st.bar_color = if window.drag_mode == DragMode::Move {
            v.window_title_fill_drag
        } else {
            v.window_title_fill
        };
        st.title_color = v.window_title_text;
        st.collapsed = window.collapsed;
        st.maximized = window.maximized;
        st.close_hovered = window.close_hovered;
        st.maximize_hovered = window.maximize_hovered;
    }
    let tb_bounds = window.title_bar.bounds();
    ctx.save();
    ctx.translate(tb_bounds.x, tb_bounds.y);
    paint_subtree(&mut window.title_bar, ctx);
    ctx.restore();

    // Outer border frames both body and title region.
    ctx.set_fill_color(v.window_fill); // restore default fill — stroke follows
    super::chrome::paint_chrome_border(ctx, w, h, &style);
}

/// Body of `<Window as Widget>::paint_overlay`: draws the resize
/// handle dots + edge highlights on top of content.
pub(super) fn paint_overlay(window: &mut Window, ctx: &mut dyn DrawCtx) {
    if !window.is_visible() || window.collapsed {
        return;
    }
    // Skip all resize-related chrome when the window can't be resized,
    // so an auto-sized or `.resizable(false)` window doesn't look
    // deceptively interactive.
    if !window.resizable || window.auto_size {
        return;
    }
    let v = ctx.visuals();
    let w = window.bounds.width;
    let h = window.bounds.height;

    // ── SE corner drag grip (3 diagonal lines, egui-style) ───────────────
    // Only shown when both axes are resizable; for uni-axis resizable
    // windows the SE grip would suggest a capability that isn't there.
    if window.resizable_h && window.resizable_v {
        let is_se_active = matches!(window.drag_mode, DragMode::Resize(ResizeDir::SE));
        let is_se_hover = window.hover_dir == Some(ResizeDir::SE);
        let grip_color = if is_se_active {
            v.window_resize_active
        } else if is_se_hover {
            v.window_resize_hover
        } else {
            v.window_stroke
        };
        ctx.set_stroke_color(grip_color);
        ctx.set_line_width(1.5);
        let m = 3.0_f64; // margin from corner edge
        for i in 1..=3_i32 {
            let off = i as f64 * 4.0 + m;
            ctx.begin_path();
            ctx.move_to(w - off, m);
            ctx.line_to(w - m, off);
            ctx.stroke();
        }
    }

    // ── Resize edge / corner highlight ────────────────────────────────────
    // Determine the highlighted direction and whether it is actively dragging.
    let (highlight, is_active) = match window.drag_mode {
        DragMode::Resize(d) => (Some(d), true),
        DragMode::Move => (None, false), // no edge highlight while moving
        DragMode::None => (window.hover_dir, false),
    };
    let dir = match highlight {
        Some(d) => d,
        None => return,
    };

    let color = if is_active {
        v.window_resize_active
    } else {
        v.window_resize_hover
    };
    ctx.set_stroke_color(color);
    ctx.set_line_width(2.0);

    // Which edges to highlight (derived from direction).
    let (top, bottom, left, right) = match dir {
        ResizeDir::N => (true, false, false, false),
        ResizeDir::S => (false, true, false, false),
        ResizeDir::E => (false, false, false, true),
        ResizeDir::W => (false, false, true, false),
        ResizeDir::NE => (true, false, false, true),
        ResizeDir::NW => (true, false, true, false),
        ResizeDir::SE => (false, true, false, true),
        ResizeDir::SW => (false, true, true, false),
    };

    // Segments run between the rounded-corner tangent points.
    let cr = CORNER_R;
    if top {
        ctx.begin_path();
        ctx.move_to(cr, h);
        ctx.line_to(w - cr, h);
        ctx.stroke();
    }
    if bottom {
        ctx.begin_path();
        ctx.move_to(cr, 0.0);
        ctx.line_to(w - cr, 0.0);
        ctx.stroke();
    }
    if left {
        ctx.begin_path();
        ctx.move_to(0.0, cr);
        ctx.line_to(0.0, h - cr);
        ctx.stroke();
    }
    if right {
        ctx.begin_path();
        ctx.move_to(w, cr);
        ctx.line_to(w, h - cr);
        ctx.stroke();
    }
}

/// Body of `<Window as Widget>::finish_paint`.  Pops the
/// compositing layer pushed in `paint` if it was opened this frame.
pub(super) fn finish_paint(window: &mut Window, ctx: &mut dyn DrawCtx) {
    if window.foreground_layer_active.replace(false) {
        ctx.pop_layer();
    }
}
