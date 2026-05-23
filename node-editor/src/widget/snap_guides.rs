//! Snap-guide painter for the node canvas.
//!
//! Lives in its own module so `widget/mod.rs` stays under the
//! 800-line limit.  Called from `NodeEditor::paint` while a node
//! drag is in flight; reads the framework's thread-local snap
//! registry to get the latest alignment / spacing guides and draws
//! them in canvas-space (the caller has already pushed the canvas
//! transform onto `ctx`).
//!
//! Color scheme matches `agg_gui::SnapOverlay` so the visual
//! language is consistent across the framework (Window snapping in
//! demo, node snapping here, future AdamArtist nodes).

use agg_gui::{snap, Color, DrawCtx, SnapGuide};

/// Alignment line tint — cyan accent for edge / centre snaps.
fn alignment_color() -> Color {
    Color::rgba(0.15, 0.70, 0.95, 0.95)
}

/// Equal-spacing dimension-line tint — pink accent.
fn spacing_color() -> Color {
    Color::rgba(0.95, 0.35, 0.55, 0.95)
}

/// Paint the current snap-guide list on `ctx`.  No-op when the
/// registry is empty (no active snap).  Caller controls when to
/// invoke us — typically only while a drag is in flight, so an idle
/// canvas doesn't repaint guides every frame.
pub(super) fn paint_snap_guides_canvas(ctx: &mut dyn DrawCtx) {
    let guides = snap::guides_snapshot();
    if guides.is_empty() {
        return;
    }
    ctx.set_line_width(1.0);
    for guide in guides {
        match guide {
            SnapGuide::VLine { x, y0, y1 } => {
                ctx.set_stroke_color(alignment_color());
                ctx.begin_path();
                ctx.move_to(x, y0);
                ctx.line_to(x, y1);
                ctx.stroke();
            }
            SnapGuide::HLine { y, x0, x1 } => {
                ctx.set_stroke_color(alignment_color());
                ctx.begin_path();
                ctx.move_to(x0, y);
                ctx.line_to(x1, y);
                ctx.stroke();
            }
            SnapGuide::HSpacing { y, x0, x1 } => {
                ctx.set_stroke_color(spacing_color());
                ctx.begin_path();
                ctx.move_to(x0, y);
                ctx.line_to(x1, y);
                ctx.stroke();
                paint_tick_v(ctx, x0, y, 4.0);
                paint_tick_v(ctx, x1, y, 4.0);
            }
            SnapGuide::VSpacing { x, y0, y1 } => {
                ctx.set_stroke_color(spacing_color());
                ctx.begin_path();
                ctx.move_to(x, y0);
                ctx.line_to(x, y1);
                ctx.stroke();
                paint_tick_h(ctx, x, y0, 4.0);
                paint_tick_h(ctx, x, y1, 4.0);
            }
        }
    }
}

fn paint_tick_v(ctx: &mut dyn DrawCtx, x: f64, y: f64, half: f64) {
    ctx.begin_path();
    ctx.move_to(x, y - half);
    ctx.line_to(x, y + half);
    ctx.stroke();
}

fn paint_tick_h(ctx: &mut dyn DrawCtx, x: f64, y: f64, half: f64) {
    ctx.begin_path();
    ctx.move_to(x - half, y);
    ctx.line_to(x + half, y);
    ctx.stroke();
}
