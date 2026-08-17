//! Icon-strip enum editor — MatterCAD's `[EnumDisplay(Mode = IconRow)]`.
//!
//! Same control as the labelled strip in [`super::enum_buttons`]: same
//! segment geometry, same hit-testing (`enum_variant_at` needs no icon
//! awareness at all), same selection chrome. Only the *contents* of a
//! segment differ — a registered [`VectorIcon`] instead of a truncated
//! label. Four operations in a ~90-px property row leave three
//! characters per segment, which is why the icon row exists: a glyph
//! that says "cut a bite out of the block" beats "Sub".
//!
//! The artwork itself is host-owned: the schema carries only icon *ids*
//! (`EditorKind::EnumIcons { variants, icons }`), and the host registers
//! the drawings once at startup via [`crate::vector_icon::register_icon`].
//! An unregistered id falls back to the variant's text so a strip is
//! never blank — a missing registration then looks like the old button
//! strip instead of an empty control the user cannot read.

use crate::vector_icon;
use crate::{DrawCtx, Rect};

use super::super::value::RowValue;
use super::enum_buttons::{paint_segment_label, paint_strip_chrome, segment_ink};

/// Logical icon side length. MatterCAD renders this family at 16×16
/// (`BooleanObject3D.cs:129-151`) and the artwork's 64-unit grid maps
/// 4:1 onto it.
const ICON_SIZE: f64 = 16.0;
/// Breathing room between the icon and its segment's edges.
const ICON_PAD: f64 = 2.0;

pub(crate) fn paint_editor(
    ctx: &mut dyn DrawCtx,
    editor_area: Rect,
    value: RowValue,
    variants: &[std::sync::Arc<str>],
    icons: &[std::sync::Arc<str>],
    scale: f64,
) {
    for (i, (r, selected)) in paint_strip_chrome(ctx, editor_area, value, variants, scale)
        .into_iter()
        .enumerate()
    {
        let art = icons.get(i).and_then(|id| vector_icon::icon(id));
        match art {
            Some(icon) => {
                let ink = segment_ink(&*ctx, selected);
                icon.paint(ctx, icon_rect(r, scale), ink);
            }
            // No artwork registered under that id (or no id at all for
            // this variant): show the name rather than nothing.
            None => paint_segment_label(ctx, r, &variants[i], selected, scale),
        }
    }
}

/// The square an icon occupies inside its segment: 16×16 logical,
/// shrunk when the segment is narrower than that, centred both ways.
fn icon_rect(segment: Rect, scale: f64) -> Rect {
    let side = (ICON_SIZE * scale)
        .min(segment.width - ICON_PAD * 2.0 * scale)
        .min(segment.height - ICON_PAD * 2.0 * scale)
        .max(0.0);
    Rect::new(
        segment.x + (segment.width - side) * 0.5,
        segment.y + (segment.height - side) * 0.5,
        side,
        side,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_centred_in_its_segment() {
        let seg = Rect::new(100.0, 10.0, 30.0, 18.0);
        let r = icon_rect(seg, 1.0);
        assert!((r.x + r.width * 0.5 - (seg.x + seg.width * 0.5)).abs() < 1e-9);
        assert!((r.y + r.height * 0.5 - (seg.y + seg.height * 0.5)).abs() < 1e-9);
        assert!((r.width - r.height).abs() < 1e-9, "icons stay square");
    }

    /// A 22-px row leaves an 18-px pill: the icon shrinks to fit rather
    /// than spilling over the segment's chrome.
    #[test]
    fn a_short_segment_shrinks_the_icon_instead_of_overflowing() {
        let seg = Rect::new(0.0, 0.0, 30.0, 18.0);
        let r = icon_rect(seg, 1.0);
        assert!(r.height <= seg.height - 2.0 * ICON_PAD);
        assert!(r.width <= r.height + 1e-9);
    }

    #[test]
    fn a_narrow_segment_clamps_to_the_width() {
        let seg = Rect::new(0.0, 0.0, 8.0, 18.0);
        let r = icon_rect(seg, 1.0);
        assert!((r.width - 4.0).abs() < 1e-9, "got {}", r.width);
    }

    /// Scale multiplies the icon the same way it multiplies the row.
    #[test]
    fn scale_multiplies_the_icon_size() {
        let seg = Rect::new(0.0, 0.0, 60.0, 44.0);
        assert!((icon_rect(seg, 2.0).width - 32.0).abs() < 1e-9);
    }
}
