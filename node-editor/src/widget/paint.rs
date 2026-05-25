//! Paint helpers for [`NodeEditor`].
//!
//! Split out of `widget/mod.rs` so the entry stays under the 800-line
//! guardrail. As a submodule of `widget`, this file retains direct
//! access to `NodeEditor`'s private fields and helper methods.
//!
//! Two entry points:
//! - [`NodeEditor::paint_canvas`] — body of the `Widget::paint` impl.
//! - [`NodeEditor::finish_paint_canvas`] — body of the `Widget::finish_paint` impl.

use agg_gui::widget::paint_subtree;
use agg_gui::{DrawCtx, Size};

use crate::draw::{draw_bezier_connection, draw_canvas_grid, CanvasPalette};

use super::hover;
use super::snap_guides::paint_snap_guides_canvas;
use super::{CanvasState, NodeEditor, SocketSide};

impl NodeEditor {
    pub(super) fn paint_canvas(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        if w <= 0.0 || h <= 0.0 {
            return;
        }

        // Refresh palette per frame — theme switches flow through.
        let visuals = ctx.visuals();
        self.palette = CanvasPalette::from_visuals(&visuals);

        if let Some(f) = agg_gui::font_settings::current_system_font() {
            ctx.set_font(f);
        }

        // Outer save: pinned by `finish_paint`.  Without it, nodes drawn
        // at canvas-y > self.bounds.height bleed into the sibling pane
        // above when a splitter shrinks the canvas.
        ctx.save();
        ctx.clip_rect(0.0, 0.0, w, h);

        ctx.set_fill_color(self.palette.canvas_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // Grid + edges live in canvas-space.  Push the canvas
        // transform (`screen = canvas * scale + offset`) on `ctx` for
        // those paints, but pop it BEFORE returning so the framework's
        // child paint pass sees the editor's normal local space —
        // NodeWidget bounds are already in screen-space (pre-baked by
        // `layout`) so they don't want this transform composed on
        // top.
        //
        // Order matters: `translate(offset)` first, then `scale(s)`.
        // ctx transforms right-multiply, so `T then S` produces the
        // matrix `T * S` — applied to a canvas point P: `T * S * P =
        // scale * P + offset`, exactly matching the screen position
        // `NodeWidget::from_layout_transformed` bakes for the
        // socket-dot widgets. Reversing the order applies `offset *
        // scale` instead and the bezier endpoints drift away from the
        // dots whenever `canvas_offset` is non-zero or `canvas_scale`
        // is not 1.
        ctx.save();
        ctx.translate(self.canvas_offset[0], self.canvas_offset[1]);
        ctx.scale(self.canvas_scale, self.canvas_scale);

        let inv_scale = 1.0 / self.canvas_scale;
        let visible_min = [
            (0.0 - self.canvas_offset[0]) * inv_scale,
            (0.0 - self.canvas_offset[1]) * inv_scale,
        ];
        let visible_max = [
            (w - self.canvas_offset[0]) * inv_scale,
            (h - self.canvas_offset[1]) * inv_scale,
        ];

        draw_canvas_grid(
            ctx,
            (visible_min, visible_max),
            40.0,
            self.palette.canvas_grid,
        );

        // Edges (under nodes).  Re-snapshot here rather than caching the
        // `layouts` from `layout()` so paint doesn't carry a hidden
        // dependency on layout-time data — the backbuffer caches the
        // result anyway, so paint runs once per real change.
        let layouts = self.snapshot_layouts();
        let model = self.model.lock().unwrap();
        let noodles = model.noodles();
        for noodle in &noodles {
            if let Some((f, t)) = hover::resolve_noodle_endpoints(&layouts, noodle) {
                let col = model.socket_color(f.socket_type);
                draw_bezier_connection(ctx, f.center, t.center, col, 2.0);
            }
        }

        // Live in-progress connection — the dangling end snaps to a
        // compatible socket under the cursor when one is in reach. The
        // snapped socket also gets a halo ring so the user has clear
        // feedback that a release here will land.
        if let CanvasState::DrawingConnection {
            from_canvas,
            cursor_canvas,
            from_socket_type,
            from_node,
            from_side,
            ..
        } = &self.interaction
        {
            let hover = hover::find_compatible_socket_near(
                &layouts,
                &*model,
                *cursor_canvas,
                *from_node,
                *from_side,
                *from_socket_type,
            );
            let endpoint = match &hover {
                Some(s) => s.center,
                None => *cursor_canvas,
            };
            let mut col = model.socket_color(*from_socket_type);
            col.a *= 0.85;
            // `draw_bezier_connection` assumes `from` is the Output
            // side (control point extends right) and `to` is the
            // Input side (control point extends left). When the user
            // drags FROM an Input socket, swap the args so the bezier
            // leaves the input going outward (left) and approaches
            // the cursor as if the cursor were the output side.
            // Mirrors the standard output→input curve shape.
            let (line_from, line_to) = match from_side {
                SocketSide::Output => (*from_canvas, endpoint),
                SocketSide::Input => (endpoint, *from_canvas),
            };
            draw_bezier_connection(ctx, line_from, line_to, col, 2.0);
            if let Some(s) = &hover {
                // Halo ring at the prospective drop target.
                let halo = model.socket_color(s.socket_type);
                ctx.set_stroke_color(halo);
                ctx.set_line_width(2.0);
                ctx.begin_path();
                let r = crate::draw::SOCKET_RADIUS * 2.0;
                ctx.circle(s.center[0], s.center[1], r);
                ctx.stroke();
            }
        }
        drop(model);

        // Snap-guide overlay — only paints while a node drag is in
        // progress.  Coordinates come from the thread-local snap
        // registry in canvas-space, matching the current ctx
        // transform, so guides land exactly on the moving node's
        // would-be edges and reference rects.
        if matches!(self.interaction, CanvasState::DraggingNode { .. }) {
            paint_snap_guides_canvas(ctx);
        }

        // Pop the canvas-space transform so the framework recurses into
        // child NodeWidgets in widget-local space — their bounds are
        // already in screen-space (pre-baked by layout()).
        ctx.restore();
    }

    pub(super) fn finish_paint_canvas(&mut self, ctx: &mut dyn DrawCtx) {
        // Popup paints in widget-local space, on top of nodes & edges
        // but inside the canvas clip.
        if self.popup.is_open() {
            if let Some(font) = agg_gui::font_settings::current_system_font() {
                let viewport = Size::new(self.bounds.width, self.bounds.height);
                self.popup.paint(ctx, font, 13.0, viewport);
            }
        }

        // Floating overlay (Window-wrapped color picker, etc.).  Painted
        // last so it sits above nodes, edges, and the popup menu.
        if let Some(overlay) = self.overlay.as_mut() {
            let b = overlay.bounds();
            if b.width > 0.0 && b.height > 0.0 {
                ctx.save();
                ctx.translate(b.x, b.y);
                paint_subtree(overlay.as_mut(), ctx);
                ctx.restore();
            }
        }

        // Pop the outer clip save.
        ctx.restore();
    }
}
