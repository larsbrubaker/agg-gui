//! Inspector properties pane painter.
//!
//! Extracted from `inspector.rs` to keep both files within the project's
//! 800-line limit.  The single entry point is [`paint_properties`], which
//! renders the lower half of the inspector panel: section header, the static
//! geometry rows (x/y/width/height/depth), editable margin and anchor rows,
//! the widget-specific properties list, and the box-model preview.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;
use crate::text::Font;
use crate::widget::InspectorNode;

use super::inspector::{
    c_border, c_dim_text, c_text, InsetsSide, InsetsTarget, PropHit, PropHitKind, FONT_SIZE,
};

pub(super) fn paint_properties(
    ctx: &mut dyn DrawCtx,
    available_h: f64,
    _panel_y_offset: f64,
    panel_w: f64,
    font: &Arc<Font>,
    selected: Option<usize>,
    nodes: &[InspectorNode],
    hits: &mut Vec<PropHit>,
) {
    if available_h < 4.0 {
        return;
    }
    let w = panel_w;
    let v = ctx.visuals().clone();

    ctx.set_font(Arc::clone(font));
    ctx.set_font_size(10.0);
    ctx.set_fill_color(c_dim_text(&v));
    ctx.fill_text("PROPERTIES", 10.0, available_h - 14.0);

    ctx.set_stroke_color(c_border(&v));
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(10.0 + 70.0, available_h - 10.0);
    ctx.line_to(w - 8.0, available_h - 10.0);
    ctx.stroke();

    let Some(sel_idx) = selected else {
        ctx.set_font_size(FONT_SIZE);
        ctx.set_fill_color(c_dim_text(&v));
        ctx.fill_text("(select a widget)", 10.0, available_h - 36.0);
        return;
    };

    let Some(node) = nodes.get(sel_idx) else {
        return;
    };

    ctx.set_font_size(14.0);
    ctx.set_fill_color(c_text(&v));
    ctx.fill_text(node.type_name, 10.0, available_h - 36.0);

    ctx.set_font_size(FONT_SIZE);
    let row_h = 18.0;
    let mut ry = available_h - 56.0;

    // ── static geometry rows ──────────────────────────────────────────────────
    let b = &node.screen_bounds;
    let geom_rows: &[(&str, String)] = &[
        ("x", format!("{:.1}", b.x)),
        ("y", format!("{:.1}", b.y)),
        ("width", format!("{:.1}", b.width)),
        ("height", format!("{:.1}", b.height)),
        ("depth", format!("{}", node.depth)),
    ];
    for (label, value) in geom_rows {
        if ry < 4.0 {
            break;
        }
        paint_row_static(ctx, w, ry, label, value, &v);
        ry -= row_h;
    }

    // ── editable margin rows (per side) ──────────────────────────────────────
    let m = &node.margin;
    let margin_sides: &[(&str, InsetsSide, f64)] = &[
        ("margin.left", InsetsSide::Left, m.left),
        ("margin.right", InsetsSide::Right, m.right),
        ("margin.top", InsetsSide::Top, m.top),
        ("margin.bottom", InsetsSide::Bottom, m.bottom),
    ];
    for (label, side, val) in margin_sides {
        if ry < 4.0 {
            break;
        }
        let step = (val.abs() * 0.5 + 0.5).min(4.0).max(0.5);
        let hit = paint_row_editable(
            ctx,
            w,
            ry,
            label,
            &format!("{:.1}", val),
            &v,
            Color::rgba(0.9, 0.55, 0.1, 0.15),
        );
        hits.push(PropHit {
            rect: hit,
            field: (*label).to_string(),
            kind: PropHitKind::InsetField {
                target: InsetsTarget::Margin,
                side: *side,
                current: *val,
                step,
            },
        });
        ry -= row_h;
    }

    // ── read-only padding rows (nonzero sides only) ───────────────────────────
    let p = &node.padding;
    if p.left != 0.0 || p.right != 0.0 || p.top != 0.0 || p.bottom != 0.0 {
        let pad_sides: &[(&str, f64)] = &[
            ("pad.left", p.left),
            ("pad.right", p.right),
            ("pad.top", p.top),
            ("pad.bottom", p.bottom),
        ];
        for (label, val) in pad_sides {
            if ry < 4.0 {
                break;
            }
            paint_row_tinted(
                ctx,
                w,
                ry,
                label,
                &format!("{:.1}", val),
                &v,
                Color::rgba(0.1, 0.75, 0.3, 0.12),
            );
            ry -= row_h;
        }
    }

    // ── editable anchor rows ──────────────────────────────────────────────────
    {
        let ha = node.h_anchor;
        if ry >= 4.0 {
            let hit = paint_row_editable(
                ctx,
                w,
                ry,
                "h_anchor",
                ha.display_name(),
                &v,
                Color::rgba(0.2, 0.4, 0.9, 0.12),
            );
            hits.push(PropHit {
                rect: hit,
                field: "h_anchor".to_string(),
                kind: PropHitKind::HAnchorCycle {
                    current_bits: ha.bits(),
                },
            });
            ry -= row_h;
        }
        let va = node.v_anchor;
        if ry >= 4.0 {
            let hit = paint_row_editable(
                ctx,
                w,
                ry,
                "v_anchor",
                va.display_name(),
                &v,
                Color::rgba(0.2, 0.4, 0.9, 0.12),
            );
            hits.push(PropHit {
                rect: hit,
                field: "v_anchor".to_string(),
                kind: PropHitKind::VAnchorCycle {
                    current_bits: va.bits(),
                },
            });
            ry -= row_h;
        }
    }

    // ── widget-specific properties ────────────────────────────────────────────
    for (prop_label, prop_value) in &node.properties {
        if ry < 4.0 {
            break;
        }
        let is_bool = *prop_value == "true" || *prop_value == "false";
        if is_bool {
            let color = if *prop_value == "true" {
                Color::rgb(0.10, 0.52, 0.10)
            } else {
                Color::rgb(0.65, 0.18, 0.18)
            };
            paint_row_colored_value(ctx, w, ry, prop_label, prop_value, color, &v);
            let hit_rect = Rect::new(w * 0.5, ry - 4.0, w * 0.5 - 2.0, 16.0);
            hits.push(PropHit {
                rect: hit_rect,
                field: (*prop_label).to_string(),
                kind: PropHitKind::BoolToggle {
                    current: *prop_value == "true",
                },
            });
        } else if let Ok(parsed) = prop_value.parse::<f64>() {
            paint_row_static(ctx, w, ry, prop_label, prop_value, &v);
            let mag = parsed.abs().max(1.0);
            let step = (mag * 0.05).max(0.1);
            let hit_rect = Rect::new(w * 0.5, ry - 4.0, w * 0.5 - 2.0, 16.0);
            hits.push(PropHit {
                rect: hit_rect,
                field: (*prop_label).to_string(),
                kind: PropHitKind::NumericStep {
                    current: parsed,
                    step,
                },
            });
        } else {
            paint_row_static(ctx, w, ry, prop_label, prop_value, &v);
        }
        ry -= row_h;
    }

    // ── box-model mini diagram ────────────────────────────────────────────────
    let diag_h = (ry - 8.0).min(80.0);
    if diag_h > 30.0 {
        let diag_y_top = diag_h - 4.0;
        let diag_w = w - 20.0;
        let aspect = if b.height > 0.0 {
            b.width / b.height
        } else {
            1.0
        };
        let box_h = (diag_h * 0.6).min(50.0);
        let box_w = (box_h * aspect).min(diag_w * 0.8);
        let box_x = 10.0 + (diag_w - box_w) * 0.5;
        let box_y = diag_y_top - (diag_h + box_h) * 0.5;

        ctx.set_fill_color(Color::rgba(0.10, 0.50, 1.0, 0.10));
        ctx.begin_path();
        ctx.rect(box_x, box_y, box_w, box_h);
        ctx.fill();
        ctx.set_stroke_color(Color::rgba(0.10, 0.50, 1.0, 0.50));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rect(box_x, box_y, box_w, box_h);
        ctx.stroke();

        let dim = format!("{:.0} × {:.0}", b.width, b.height);
        ctx.set_font_size(10.0);
        ctx.set_fill_color(Color::rgba(0.10, 0.40, 0.90, 0.80));
        if let Some(m) = ctx.measure_text(&dim) {
            if m.width < box_w - 4.0 {
                ctx.fill_text(
                    &dim,
                    box_x + (box_w - m.width) * 0.5,
                    box_y + (box_h - m.ascent - m.descent) * 0.5 + m.descent,
                );
            }
        }
    }
}

// ── row drawing helpers ───────────────────────────────────────────────────────

fn paint_separator(ctx: &mut dyn DrawCtx, w: f64, ry: f64, v: &crate::theme::Visuals) {
    ctx.set_stroke_color(c_border(v));
    ctx.set_line_width(0.5);
    ctx.begin_path();
    ctx.move_to(8.0, ry - 4.0);
    ctx.line_to(w - 8.0, ry - 4.0);
    ctx.stroke();
}

fn paint_row_static(
    ctx: &mut dyn DrawCtx,
    w: f64,
    ry: f64,
    label: &str,
    value: &str,
    v: &crate::theme::Visuals,
) {
    ctx.set_fill_color(c_dim_text(v));
    ctx.fill_text(label, 12.0, ry);
    ctx.set_fill_color(c_text(v));
    if let Some(m) = ctx.measure_text(value) {
        ctx.fill_text(value, w - m.width - 10.0, ry);
    }
    paint_separator(ctx, w, ry, v);
}

fn paint_row_colored_value(
    ctx: &mut dyn DrawCtx,
    w: f64,
    ry: f64,
    label: &str,
    value: &str,
    color: Color,
    v: &crate::theme::Visuals,
) {
    ctx.set_fill_color(c_dim_text(v));
    ctx.fill_text(label, 12.0, ry);
    ctx.set_fill_color(color);
    if let Some(m) = ctx.measure_text(value) {
        ctx.fill_text(value, w - m.width - 10.0, ry);
    }
    paint_separator(ctx, w, ry, v);
}

/// Paint a row whose value has a tinted background rectangle — indicates the
/// field is editable (click left half to decrement, right half to increment).
/// Returns the hit rectangle.
fn paint_row_editable(
    ctx: &mut dyn DrawCtx,
    w: f64,
    ry: f64,
    label: &str,
    value: &str,
    v: &crate::theme::Visuals,
    tint: Color,
) -> Rect {
    let hit_x = w * 0.5;
    let hit_w = w * 0.5 - 2.0;
    // Tinted background for the value half
    ctx.set_fill_color(tint);
    ctx.begin_path();
    ctx.rect(hit_x, ry - 3.0, hit_w, 14.0);
    ctx.fill();

    ctx.set_fill_color(c_dim_text(v));
    ctx.fill_text(label, 12.0, ry);
    ctx.set_fill_color(c_text(v));
    if let Some(m) = ctx.measure_text(value) {
        ctx.fill_text(value, w - m.width - 10.0, ry);
    }
    paint_separator(ctx, w, ry, v);
    Rect::new(hit_x, ry - 4.0, hit_w, 16.0)
}

/// Like `paint_row_editable` but uses a different tint and does NOT register a
/// hit rect (read-only display with visual distinction from plain rows).
fn paint_row_tinted(
    ctx: &mut dyn DrawCtx,
    w: f64,
    ry: f64,
    label: &str,
    value: &str,
    v: &crate::theme::Visuals,
    tint: Color,
) {
    let hit_x = w * 0.5;
    let hit_w = w * 0.5 - 2.0;
    ctx.set_fill_color(tint);
    ctx.begin_path();
    ctx.rect(hit_x, ry - 3.0, hit_w, 14.0);
    ctx.fill();

    ctx.set_fill_color(c_dim_text(v));
    ctx.fill_text(label, 12.0, ry);
    ctx.set_fill_color(c_text(v));
    if let Some(m) = ctx.measure_text(value) {
        ctx.fill_text(value, w - m.width - 10.0, ry);
    }
    paint_separator(ctx, w, ry, v);
}
