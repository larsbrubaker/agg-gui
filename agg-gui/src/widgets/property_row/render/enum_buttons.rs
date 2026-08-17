//! Enum editor renderer — a segmented button strip, one segment per
//! variant, the current value filled with the accent colour.
//!
//! Serves `EditorKind::EnumButtons`, `EnumTabs` and `EnumDropdown`: all
//! three are "pick one of a short, fixed list", and a strip reads the
//! same in a 22-px property row whichever presentation the schema asked
//! for. A real popup list for `EnumDropdown` is follow-up work; adding
//! it follows the three-step recipe in [`super`]'s module docs (variant,
//! file, dispatch arm).
//!
//! `EditorKind::EnumIcons` is the same strip with artwork instead of
//! labels — it lives in [`super::enum_icons`] and reuses this module's
//! geometry and chrome ([`paint_strip_chrome`], [`segment_ink`],
//! [`paint_segment_label`] for its fallback) so the two presentations
//! cannot drift apart.
//!
//! Hosts route clicks through [`variant_at`] (re-exported as
//! `enum_variant_at`), which recomputes exactly the geometry
//! [`paint_editor`] draws, so "what the user pointed at" and "what got
//! highlighted" can't drift apart.

use crate::{Color, DrawCtx, Rect};

use super::super::value::RowValue;
use super::{editor_pill_rect, paint_pill_bg};

/// Per-segment inner padding, at scale = 1.
const SEGMENT_PAD: f64 = 3.0;
/// Estimated glyph width at the row font size, at scale = 1. Same
/// constant the other renderers use for centring short strings.
const GLYPH_WIDTH: f64 = 6.5;

/// The rectangle of each variant's segment, in the same space as
/// `editor_area`. Empty when `count` is 0.
pub(crate) fn segment_rects(editor_area: Rect, count: usize, scale: f64) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    let pill = editor_pill_rect(editor_area, scale);
    let seg_w = pill.width / count as f64;
    (0..count)
        .map(|i| Rect::new(pill.x + seg_w * i as f64, pill.y, seg_w, pill.height))
        .collect()
}

/// Index of the segment containing `x` (same space as `editor_area`),
/// or `None` when the point is outside the strip horizontally.
///
/// **X only, deliberately.** The painted pill is inset a couple of
/// pixels inside the row, but the click target is the row's full height:
/// a 22-px row leaves a ~18-px pill, and refusing the two-pixel margin
/// would make the strip feel like it has dead edges. The caller has
/// already established that the pointer is inside this row.
pub(crate) fn variant_at(editor_area: Rect, count: usize, x: f64, scale: f64) -> Option<usize> {
    segment_rects(editor_area, count, scale)
        .into_iter()
        .position(|r| x >= r.x && x <= r.x + r.width)
}

/// Paint the strip's background and per-segment chrome (the accent fill
/// under the current value, hairline separators between the rest) and
/// hand back each segment's rect paired with "is this the selected one".
///
/// Shared with the icon strip (`super::enum_icons`) so both
/// presentations are the same control with different segment contents —
/// if they painted their own chrome, selection would eventually look
/// different depending on which editor kind the schema asked for.
pub(crate) fn paint_strip_chrome(
    ctx: &mut dyn DrawCtx,
    editor_area: Rect,
    value: RowValue,
    variants: &[std::sync::Arc<str>],
    scale: f64,
) -> Vec<(Rect, bool)> {
    let pill = editor_pill_rect(editor_area, scale);
    paint_pill_bg(ctx, pill, scale);
    if variants.is_empty() {
        return Vec::new();
    }

    let current = value.as_short_text().unwrap_or("");
    let selected = variants.iter().position(|v| v.as_ref() == current);
    let visuals = ctx.visuals().clone();
    let rects = segment_rects(editor_area, variants.len(), scale);

    for (i, r) in rects.iter().enumerate() {
        if Some(i) == selected {
            ctx.set_fill_color(visuals.accent);
            ctx.begin_path();
            ctx.rounded_rect(
                r.x + SEGMENT_PAD * 0.5 * scale,
                r.y + SEGMENT_PAD * 0.5 * scale,
                (r.width - SEGMENT_PAD * scale).max(0.0),
                (r.height - SEGMENT_PAD * scale).max(0.0),
                2.0 * scale,
            );
            ctx.fill();
        } else if i > 0 {
            // Hairline separator between unselected segments so the
            // strip reads as several buttons rather than one pill.
            ctx.set_stroke_color(visuals.window_stroke);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.move_to(r.x, r.y + 2.0 * scale);
            ctx.line_to(r.x, r.y + r.height - 2.0 * scale);
            ctx.stroke();
        }
    }

    rects
        .into_iter()
        .enumerate()
        .map(|(i, r)| (r, Some(i) == selected))
        .collect()
}

/// The colour a segment's contents should use — light on the accent
/// fill, body text elsewhere. Shared with the icon strip so an icon's
/// ink and a label's ink agree.
pub(crate) fn segment_ink(ctx: &dyn DrawCtx, selected: bool) -> Color {
    if selected {
        // The accent fill is dark in both themes; keep the selected
        // content readable rather than inheriting the dim body text.
        Color::rgba(0.98, 0.98, 0.98, 1.0)
    } else {
        ctx.visuals().text_color
    }
}

/// Draw a variant's (possibly truncated) label centred in `r`.
pub(crate) fn paint_segment_label(
    ctx: &mut dyn DrawCtx,
    r: Rect,
    variant: &str,
    selected: bool,
    scale: f64,
) {
    let label = fit_label(variant, r.width - SEGMENT_PAD * 2.0 * scale, scale);
    if label.is_empty() {
        return;
    }
    let ink = segment_ink(ctx, selected);
    ctx.set_fill_color(ink);
    ctx.set_font_size(11.0 * scale);
    let est_w = label.chars().count() as f64 * GLYPH_WIDTH * scale;
    let text_x = (r.x + (r.width - est_w) * 0.5).max(r.x + 2.0 * scale);
    let text_y = r.y + r.height * 0.5 - 4.0 * scale;
    ctx.fill_text(&label, text_x, text_y);
}

pub(crate) fn paint_editor(
    ctx: &mut dyn DrawCtx,
    editor_area: Rect,
    value: RowValue,
    variants: &[std::sync::Arc<str>],
    scale: f64,
) {
    for (i, (r, selected)) in paint_strip_chrome(ctx, editor_area, value, variants, scale)
        .into_iter()
        .enumerate()
    {
        paint_segment_label(ctx, r, &variants[i], selected, scale);
    }
}

/// Shorten a variant name to what fits in `width`. Four operations in a
/// 90-px strip leave room for two or three characters, so the fallback
/// is a prefix rather than an ellipsis — "Sub", "Int" still tell the
/// variants apart, "…" does not.
fn fit_label(variant: &str, width: f64, scale: f64) -> String {
    if width <= 0.0 {
        return String::new();
    }
    let max_chars = (width / (GLYPH_WIDTH * scale)).floor() as usize;
    if max_chars == 0 {
        return String::new();
    }
    if variant.chars().count() <= max_chars {
        return variant.to_string();
    }
    variant.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(100.0, 0.0, 80.0, 22.0)
    }

    #[test]
    fn segments_tile_the_pill_without_gaps() {
        let rects = segment_rects(area(), 4, 1.0);
        assert_eq!(rects.len(), 4);
        let pill = editor_pill_rect(area(), 1.0);
        assert!((rects[0].x - pill.x).abs() < 1e-9);
        let last = rects[3];
        assert!((last.x + last.width - (pill.x + pill.width)).abs() < 1e-9);
        for pair in rects.windows(2) {
            assert!((pair[0].x + pair[0].width - pair[1].x).abs() < 1e-9);
        }
    }

    #[test]
    fn variant_at_maps_each_quarter_to_its_index() {
        let a = area();
        let pill = editor_pill_rect(a, 1.0);
        for i in 0..4 {
            let x = pill.x + pill.width * (i as f64 + 0.5) / 4.0;
            assert_eq!(variant_at(a, 4, x, 1.0), Some(i), "at x = {}", x);
        }
    }

    #[test]
    fn variant_at_outside_the_strip_is_none() {
        let a = area();
        let pill = editor_pill_rect(a, 1.0);
        assert_eq!(variant_at(a, 4, pill.x - 5.0, 1.0), None);
        assert_eq!(variant_at(a, 4, pill.x + pill.width + 5.0, 1.0), None);
        assert_eq!(variant_at(a, 0, pill.x + 1.0, 1.0), None);
    }

    #[test]
    fn fit_label_truncates_to_a_prefix() {
        assert_eq!(fit_label("Combine", 100.0, 1.0), "Combine");
        assert_eq!(fit_label("Subtract & Replace", 20.0, 1.0), "Sub");
        assert_eq!(fit_label("Combine", 0.0, 1.0), "");
    }
}
