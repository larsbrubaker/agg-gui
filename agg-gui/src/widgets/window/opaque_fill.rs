// Fills a window's window-sized shapes (its shadow layers and its body)
// without paying for the flat middle an opaque body covers. Port of
// agg-sharp `Gui/WindowWidget/OpaqueRoundedFill.cs`.
//
// The shadow is several stacked fills the size of the whole window and the
// body is one more; on an LCD backbuffer every fill is a 3x raster, a 5-tap
// filter and a per-pixel composite over its whole clipped extent (the LCD
// mask is bounded by the clip, `LcdBuffer::fill_path`). Under an opaque body
// none of the shadow shows in the middle and the body is one flat colour, so
// `paint.rs` clears that middle once (`DrawCtx::clear_rect`) and fills each
// shape only in the four bands around it. Coverage inside a band is the
// shape's own, so the pixels are the ones the whole fills drew.
//
// agg-sharp also swaps each band's shape for a piece cut a little past the
// band, so its rasterizer does not sweep the rest of the shape; agg-gui's
// rasterizers clip the outline to the clip box before building coverage, so
// the whole shape is filled under each band's clip.

use crate::draw_ctx::DrawCtx;

/// A rectangle in the target's own pixels (y up): left, bottom, right, top.
pub(super) type DeviceRect = (f64, f64, f64, f64);

/// The whole device pixels a rounded rect of `radius` over `(x, y, w, h)`
/// (in the ctx's current coordinates) covers completely, or `None` when
/// that is not simply worked out (a rotated or flipped transform) or is
/// empty. Pulled in by the radius plus a pixel, so the pixels along its
/// edge are fully covered too.
///
/// agg-sharp inflates by `-(radius + 1)` and takes the min and max of the
/// transformed corners, so a rect narrower than `2 * (radius + 1)` came out
/// turned inside out with a positive size, an "interior" the shape does not
/// cover. Here a rect with no inside left after the inset has no interior.
pub(super) fn solid_interior(
    ctx: &dyn DrawCtx,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radius: f64,
) -> Option<DeviceRect> {
    let t = ctx.transform();
    if t.shx != 0.0 || t.shy != 0.0 || t.sx <= 0.0 || t.sy <= 0.0 {
        return None;
    }
    let inset = radius.max(0.0) + 1.0;
    let (mut x1, mut y1) = (x + inset, y + inset);
    let (mut x2, mut y2) = (x + w - inset, y + h - inset);
    if x2 <= x1 || y2 <= y1 {
        return None;
    }
    t.transform(&mut x1, &mut y1);
    t.transform(&mut x2, &mut y2);
    let interior = (
        x1.min(x2).ceil(),
        y1.min(y2).ceil(),
        x1.max(x2).floor(),
        y1.max(y2).floor(),
    );
    (interior.2 > interior.0 && interior.3 > interior.1).then_some(interior)
}

/// The device bounds of the local rect `(x, y, w, h)`, grown to whole pixels
/// and one more for anti-aliasing: every pixel a fill of a shape inside the
/// rect can touch.
pub(super) fn device_bounds(ctx: &dyn DrawCtx, x: f64, y: f64, w: f64, h: f64) -> DeviceRect {
    let t = ctx.transform();
    let mut r = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for (mut px, mut py) in [(x, y), (x + w, y), (x, y + h), (x + w, y + h)] {
        t.transform(&mut px, &mut py);
        r = (r.0.min(px), r.1.min(py), r.2.max(px), r.3.max(py));
    }
    (
        r.0.floor() - 1.0,
        r.1.floor() - 1.0,
        r.2.ceil() + 1.0,
        r.3.ceil() + 1.0,
    )
}

/// Fill the path `build` appends, which lies inside the device rect
/// `bounds`, everywhere but the device rect `skip`, one band around it at a
/// time; with no rect to skip, fill all of it. The fill colour is the one
/// already set.
///
/// The bands reach a pixel into `skip`: past a clip the LCD filter reads
/// nothing, so a band's last pixel comes out partly covered, and inside the
/// skipped rect that pixel lands on the flat colour it would have had anyway.
pub(super) fn fill_around(
    ctx: &mut dyn DrawCtx,
    bounds: DeviceRect,
    skip: Option<DeviceRect>,
    build: &dyn Fn(&mut dyn DrawCtx),
) {
    let hole = skip.map(|s| (s.0 + 1.0, s.1 + 1.0, s.2 - 1.0, s.3 - 1.0));
    let Some(hole) = hole.filter(|h| h.2 > h.0 && h.3 > h.1) else {
        ctx.begin_path();
        build(ctx);
        ctx.fill();
        return;
    };
    let (l, b, r, t) = bounds;
    let bands = [
        (l, hole.3, r, t),
        (l, b, r, hole.1),
        (l, hole.1, hole.0, hole.3),
        (hole.2, hole.1, r, hole.3),
    ];
    let local = ctx.transform();
    for band in bands {
        if band.2 <= band.0 || band.3 <= band.1 {
            continue;
        }
        // The band is clipped in the target's own pixels (as
        // `DrawCtx::clear_rect` does), so neighbouring bands meet on exact
        // pixel edges and no row is filled twice or left out.
        ctx.save();
        ctx.reset_transform();
        ctx.clip_rect(band.0, band.1, band.2 - band.0, band.3 - band.1);
        ctx.set_transform(local);
        ctx.begin_path();
        build(ctx);
        ctx.fill();
        ctx.restore();
    }
}
