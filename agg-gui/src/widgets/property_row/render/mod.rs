//! Per-`EditorKind` row renderers.
//!
//! Each editor variant has its own file in this directory. The
//! [`paint_row`] dispatcher inspects the [`EditorKind`] and forwards
//! to the variant's painter. Adding a new editor type means:
//!
//!   1. Add the variant to [`EditorKind`](super::EditorKind).
//!   2. Add `mod foo;` + a `pub(crate) fn paint(...)` in this module.
//!   3. Wire one arm in the dispatcher.
//!
//! The renderers never own state — each call paints from scratch.
//! That keeps them safe to share across host crates and easy to
//! reason about: every paint is a pure function of (area, label,
//! value, attrs, theme).
//!
//! ## Common layout convention
//!
//! Every renderer paints a horizontal row split into two zones:
//!
//!   - **Label zone** — left ~45%, vertically centred, single-line
//!     text in the theme's normal text colour.
//!   - **Editor zone** — right ~55%, vertically centred, kind-
//!     specific pill / control.
//!
//! Exceptions:
//!
//!   - `StringReadOnly` claims the full row width when the label is
//!     empty (the hint-message case); otherwise it follows the normal
//!     split with wrapped text on the right.
//!   - `ColorPicker` paints the swatch as a thick outlined pill so
//!     transparency reads cleanly against the panel background.

use crate::{DrawCtx, Rect};

use super::editor::{EditorKind, NumberAttrs};
use super::value::RowValue;

mod color;
mod default_row;
mod matrix;
mod slider;
mod string_read_only;
mod toggle;

/// Paint a complete row — label on the left, editor on the right.
/// Used for unbound property rows where the renderer owns the whole
/// row.
///
/// Bound input rows (where a sibling [`RowLabelWidget`] already
/// paints the row's label next to the socket dot) call
/// [`paint_editor_only`] instead — same per-kind renderer code, no
/// label.
pub fn paint_row(
    ctx: &mut dyn DrawCtx,
    area: Rect,
    label: &str,
    value: RowValue,
    editor: &EditorKind,
    scale: f64,
) {
    // Empty label = renderer gets the full row width. This is what
    // MatterCAD's `[DisplayName("")]` + `[ReadOnly]` combo on a string
    // row produces: a paragraph that wraps across the entire node
    // body. Splitting label/editor when there's no label would clip
    // the text to half the row for no reason.
    if label.is_empty() {
        dispatch_editor(ctx, area, value, editor, scale);
        return;
    }
    let (label_rect, editor_rect) = split_label_editor(area, scale);
    paint_label(ctx, label_rect, label, scale);
    dispatch_editor(ctx, editor_rect, value, editor, scale);
}

/// Paint only the editor portion of a row. The host paints the
/// label elsewhere (typically a sibling label widget on the input
/// socket's row). Same per-kind renderers as [`paint_row`] — single
/// code path, just no label.
pub fn paint_editor_only(
    ctx: &mut dyn DrawCtx,
    area: Rect,
    value: RowValue,
    editor: &EditorKind,
    scale: f64,
) {
    dispatch_editor(ctx, area, value, editor, scale);
}

/// Internal dispatch: send `(editor_area, value, attrs)` to the
/// per-`EditorKind` renderer. Every renderer file exposes a single
/// `paint_editor` function with this contract.
fn dispatch_editor(
    ctx: &mut dyn DrawCtx,
    editor_area: Rect,
    value: RowValue,
    editor: &EditorKind,
    scale: f64,
) {
    match editor {
        EditorKind::Slider(attrs) => slider::paint_editor(ctx, editor_area, value, attrs, scale),
        EditorKind::NumberDrag(attrs) => {
            slider::paint_editor_drag(ctx, editor_area, value, attrs, scale)
        }
        EditorKind::Toggle => toggle::paint_editor(ctx, editor_area, value, scale),
        EditorKind::ColorPicker => color::paint_editor(ctx, editor_area, value, scale),
        EditorKind::Matrix => matrix::paint_editor(ctx, editor_area, value, scale),
        EditorKind::StringReadOnly => {
            string_read_only::paint_editor(ctx, editor_area, value, scale)
        }
        // Other variants currently fall through to the default
        // painter — they'll get dedicated renderers in follow-up
        // work (single-/multi-line text editors, enum dropdown /
        // buttons / tabs, image picker).
        _ => default_row::paint_editor(ctx, editor_area, value, scale),
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Standard horizontal label/editor split. Returns `(label_rect,
/// editor_rect)`. The split sits at ~45% of the row width so labels
/// fit before the editor pill.
pub(crate) fn split_label_editor(area: Rect, scale: f64) -> (Rect, Rect) {
    let split = area.x + area.width * 0.45;
    let pad = 8.0 * scale;
    let label = Rect::new(
        area.x + pad,
        area.y,
        (split - area.x - pad).max(0.0),
        area.height,
    );
    let editor = Rect::new(
        split,
        area.y,
        (area.width - (split - area.x) - pad).max(0.0),
        area.height,
    );
    (label, editor)
}

/// Paint a label string left-aligned inside `area`. Vertical
/// centring matches `RowLabelWidget`'s convention — `y = midpoint -
/// 4·scale` — so labels painted by the row renderer line up with
/// labels painted by sibling widgets on the same row.
pub(crate) fn paint_label(ctx: &mut dyn DrawCtx, area: Rect, label: &str, scale: f64) {
    if label.is_empty() {
        return;
    }
    let visuals = ctx.visuals().clone();
    ctx.set_fill_color(visuals.text_color);
    ctx.set_font_size(11.0 * scale);
    let y = area.y + area.height * 0.5 - 4.0 * scale;
    ctx.fill_text(label, area.x, y);
}

/// Paint a generic editor-pill background (rounded rect). Renderers
/// that want a different shape draw their own.
pub(crate) fn paint_pill_bg(ctx: &mut dyn DrawCtx, area: Rect, scale: f64) {
    let visuals = ctx.visuals().clone();
    // The pill base reads as a faint inset against the panel — same
    // palette role as a text-field's idle background.
    ctx.set_fill_color(visuals.window_fill);
    ctx.begin_path();
    ctx.rounded_rect(area.x, area.y, area.width, area.height, 3.0 * scale);
    ctx.fill();
    ctx.set_stroke_color(visuals.window_stroke);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(area.x, area.y, area.width, area.height, 3.0 * scale);
    ctx.stroke();
}

/// Vertically-centred editor sub-rect: shrinks the editor zone's
/// height down to a typical pill size, padded inside the row.
pub(crate) fn editor_pill_rect(editor_area: Rect, scale: f64) -> Rect {
    let pill_h = (editor_area.height - 4.0 * scale).max(12.0 * scale);
    let pill_y = editor_area.y + (editor_area.height - pill_h) * 0.5;
    Rect::new(editor_area.x, pill_y, editor_area.width, pill_h)
}

pub(crate) fn format_number(n: f64, attrs: Option<&NumberAttrs>) -> String {
    let dp = attrs.and_then(|a| a.max_decimal_places);
    let integer = attrs.map(|a| a.integer).unwrap_or(false);
    if integer || n.fract().abs() < 1e-6 {
        format!("{}", n as i64)
    } else if let Some(places) = dp {
        format!("{:.*}", places as usize, n)
    } else {
        format!("{:.3}", n)
    }
}
