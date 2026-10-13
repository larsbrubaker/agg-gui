//! The formatting toolbar's icons, drawn as vector shapes on the device-pixel
//! grid (no C# counterpart: the icon set is agg-gui's own, in place of the
//! Font Awesome glyphs the C# toolbar draws).
//!
//! Why drawn rather than a font: a font glyph's bars have the heights its
//! designer chose, 1.5 px thick at some sizes, so at 1.5x they straddle two
//! pixel rows and blur.  Here every straight edge (the bars of the align and
//! list icons, the quote rule, the table's grid, the strike line, the stems of
//! B and S) is placed on whole device pixels at any scale, with one stroke
//! thickness for the whole set, so the icons are equally sharp at 1x, 1.5x and
//! 2x and read as one family.  Only the diagonals and curves (italic, the
//! chevrons, the link, B's bowls) are antialiased, as they must be.
//!
//! The icons are placed by [`paint_icon`], which a flat toolbar `Button` calls
//! through `with_icon_painter` in its own local space.  A square design box
//! sixteen units across is centred on a whole device pixel and every
//! coordinate below is in device pixels, Y down from the box's top left.

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::Size;
use agg_rust::math_stroke::{LineCap, LineJoin};

use super::format_toolbar::MarkdownFormatButtonKind as Kind;

/// The icon box's side, in design units.
const DESIGN_BOX: f64 = 16.0;
/// Bezier control distance for a quarter circle.
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// The device-pixel metrics one icon is built from.
struct Grid {
    /// The icon box's side.
    n: f64,
    /// The thickness of a straight bar.
    t: f64,
    /// The thickness of a fine line: the outline of a box, a table's rules.
    line: f64,
    /// The scale (device pixels per logical unit).
    s: f64,
}

impl Grid {
    /// The stroke width of diagonals and curves, a little lighter than a bar
    /// since slanted strokes read heavier; not snapped, as they cannot be.
    fn thin(&self) -> f64 {
        (self.s * 1.4).max(1.0)
    }
}

/// Draws the icon for `kind`, centred in a button of logical `size`, in
/// `color`.  The context is in the button's local space; it can carry any
/// scale, since the grid is read from its transform.
pub(super) fn paint_icon(ctx: &mut dyn DrawCtx, kind: Kind, size: Size, color: Color) {
    let t = ctx.transform();
    let s = if t.sx.abs() > 1e-6 {
        t.sx.abs()
    } else {
        crate::ux_scale::effective_scale()
    };
    ctx.save();
    // Whole device pixels from here on: snap the origin, then count in device units.
    ctx.snap_to_pixel();
    ctx.scale(1.0 / s, 1.0 / s);
    let n = (DESIGN_BOX * s).round();
    let grid = Grid {
        n,
        t: (s * 1.5).round().max(1.0),
        line: s.round().max(1.0),
        s,
    };
    let x0 = ((size.width * s).round() - n).div_euclid(2.0);
    let y_bottom = ((size.height * s).round() - n).div_euclid(2.0);
    // Y down from the box's top left.
    ctx.translate(x0, y_bottom + n);
    ctx.scale(1.0, -1.0);
    ctx.set_fill_color(color);
    ctx.set_stroke_color(color);
    ctx.set_line_cap(LineCap::Butt);
    ctx.set_line_join(LineJoin::Miter);
    match kind {
        Kind::Bold => bold(ctx, &grid),
        Kind::Italic => italic(ctx, &grid),
        Kind::Strike => strike(ctx, &grid),
        Kind::Code => code(ctx, &grid),
        Kind::AlignLeft | Kind::AlignCenter | Kind::AlignRight => align(ctx, &grid, kind),
        Kind::BulletList => bullets(ctx, &grid),
        Kind::NumberedList => numbers(ctx, &grid),
        Kind::Quote => quote(ctx, &grid),
        Kind::Link => link(ctx, &grid),
        Kind::CodeBlock => code_block(ctx, &grid),
        Kind::Table => table(ctx, &grid),
    }
    ctx.restore();
}

/// Fills every rectangle `(x, y, w, h)` as one shape, so overlaps do not
/// darken where the icon is drawn translucent.
fn fill_rects(ctx: &mut dyn DrawCtx, rects: &[(f64, f64, f64, f64)]) {
    ctx.begin_path();
    for &(x, y, w, h) in rects {
        ctx.rect(x, y, w, h);
    }
    ctx.fill();
}

/// Four bars, long and short, against the left, the middle or the right.
fn align(ctx: &mut dyn DrawCtx, g: &Grid, kind: Kind) {
    let pitch = ((g.n - g.t) / 3.0).floor();
    let top = ((g.n - (3.0 * pitch + g.t)) / 2.0).floor();
    // The short bars are 62% of the long, with the same parity as the box so a
    // centred one has whole pixels on both sides.
    let mut short = (g.n * 0.62).round();
    if (g.n - short) % 2.0 != 0.0 {
        short += 1.0;
    }
    let bars: Vec<_> = (0..4)
        .map(|i| {
            let w = if i % 2 == 0 { g.n } else { short };
            let x = match kind {
                Kind::AlignCenter => ((g.n - w) / 2.0).floor(),
                Kind::AlignRight => g.n - w,
                _ => 0.0,
            };
            (x, top + i as f64 * pitch, w, g.t)
        })
        .collect();
    fill_rects(ctx, &bars);
}

/// Three rows of a round bullet and a bar.  The bullet is two pixels taller
/// than the bar, so its top and bottom rows stay whole pixels.
fn bullets(ctx: &mut dyn DrawCtx, g: &Grid) {
    let pitch = ((g.n - g.t) / 2.0).floor();
    let dot = g.t + 2.0;
    let bar_x = dot + g.t.max(2.0);
    let mut rects = Vec::new();
    ctx.begin_path();
    for i in 0..3 {
        let y = i as f64 * pitch;
        ctx.rounded_rect(0.0, y - 1.0, dot, dot, dot / 2.0);
        rects.push((bar_x, y, g.n - bar_x, g.t));
    }
    ctx.fill();
    fill_rects(ctx, &rects);
}

/// The numerals 1 and 2, one above the other, each followed by a bar.  The
/// numerals are drawn from straight bars only, like a seven-segment display, so
/// they stay legible in a 16-unit box and have no half-covered pixel.  Their
/// stroke is the bars' thickness wherever two numerals with open counters fit
/// the box (1.5x and 2x); at 1x, where two-pixel strokes would close the "2" up,
/// it is one pixel.  The bars sit on the bulleted list's first and third bar
/// rows, so the two list icons share a rhythm: the first bar is level with the
/// "1"'s top and the second with the "2"'s base.  At least a stroke's width of
/// clear space separates the numerals.
fn numbers(ctx: &mut dyn DrawCtx, g: &Grid) {
    // The numeral's height for a stroke `c`, leaving a gap of one stroke and a pixel.
    let height = |c: f64| ((g.n - c - 1.0) / 2.0).floor();
    // A "2" needs its three bars plus two counters of two pixels to stay open.
    let c = if height(g.t) >= 3.0 * g.t + 4.0 {
        g.t
    } else {
        1.0
    };
    let h = height(c);
    let w = (h * 0.66).round();
    let bar_x = w + (c + 1.0).max(2.0);
    // The bulleted list's bar rows (see `bullets`).
    let pitch = ((g.n - g.t) / 2.0).floor();
    let (first_bar, last_bar) = (0.0, 2.0 * pitch);
    // The "2" ends where the last bar ends, the "1" starts where the first starts.
    let y2 = last_bar + g.t - h;
    // What is left of the height after the three bars of the 2 goes to its
    // two verticals, the upper one the shorter.
    let upper = ((h - 3.0 * c) / 2.0).floor();
    let lower = h - 3.0 * c - upper;
    let mut rects = Vec::new();
    // A 1: a stem right of the middle with a flag of one step below its top,
    // or two where the numeral is tall enough and the room is there.  It has no
    // base, which would run into the 2's top bar below it and read as a fraction.
    let stem = ((w - c) * 0.6).round();
    rects.push((stem, 0.0, c, h));
    let steps = (if h >= 10.0 { 2.0_f64 } else { 1.0 }).min((stem / c).floor());
    for step in 1..=steps as usize {
        let k = step as f64;
        rects.push((stem - k * c, k * c, c, c));
    }
    // A 2: top bar, upper right vertical, middle bar, lower left vertical, base.
    rects.push((0.0, y2, w, c));
    rects.push((w - c, y2 + c, c, upper));
    rects.push((0.0, y2 + c + upper, w, c));
    rects.push((0.0, y2 + 2.0 * c + upper, c, lower));
    rects.push((0.0, y2 + h - c, w, c));
    rects.push((bar_x, first_bar, g.n - bar_x, g.t));
    rects.push((bar_x, last_bar, g.n - bar_x, g.t));
    fill_rects(ctx, &rects);
}

/// An opening quotation mark twice, the "66" of a quote: each mark is a block
/// with a tail that rises from its left edge and curls over to a flat tip on the
/// right.  The block's left, right and bottom sides and the tip's top are
/// straight, on whole device pixels.
fn quote(ctx: &mut dyn DrawCtx, g: &Grid) {
    let gap = (g.n * 0.125).round();
    let w = ((g.n - gap) / 2.0).floor();
    let h = (g.n * 0.6).round();
    let top = ((g.n - h) / 2.0).floor();
    let x0 = ((g.n - (2.0 * w + gap)) / 2.0).floor();
    // The block's height and its corner radius; the tail's tip is as wide as
    // the block is thick.
    let bulb = (h * 0.55).round();
    let r = (g.n * 0.1).round().max(1.0);
    let tip = (w * 0.4).round().max(2.0);
    ctx.begin_path();
    for i in 0..2 {
        let x = x0 + i as f64 * (w + gap);
        let (y_bulb, bottom) = (top + h - bulb, top + h);
        // Round the block's two lower corners and its upper right.
        ctx.move_to(x, y_bulb);
        ctx.line_to(x, bottom - r);
        ctx.quad_to(x, bottom, x + r, bottom);
        ctx.line_to(x + w - r, bottom);
        ctx.quad_to(x + w, bottom, x + w, bottom - r);
        ctx.line_to(x + w, y_bulb + r);
        ctx.quad_to(x + w, y_bulb, x + w - r, y_bulb);
        // Back along the block's top to where the tail's inner edge leaves it,
        // then up that edge to the tip's right side.
        let inner = x + w * 0.55;
        ctx.line_to(inner, y_bulb);
        ctx.cubic_to(inner, y_bulb - h * 0.2, x + w, top + h * 0.25, x + w, top);
        ctx.line_to(x + w - tip, top);
        // Down and round the tail's outer edge to the block's left side.
        ctx.cubic_to(x + w * 0.2, top, x, top + h * 0.2, x, y_bulb);
        ctx.close_path();
    }
    ctx.fill();
}

/// A bold B: a stem and two bowls, the lower a little wider.
fn bold(ctx: &mut dyn DrawCtx, g: &Grid) {
    let tb = (g.t + 1.0).max((g.s * 2.6).round());
    let h = (g.n * 0.8).round();
    let counter = ((h - 3.0 * tb) / 2.0).round().max(1.0);
    let h = 3.0 * tb + 2.0 * counter;
    let w = (g.n * 0.58).round();
    let w_top = w - (g.n * 0.07).round();
    let (x0, y0) = (((g.n - w) / 2.0).floor(), ((g.n - h) / 2.0).floor());
    let (left, top) = (x0 + tb / 2.0, y0 + tb / 2.0);
    let mid = y0 + tb + counter + tb / 2.0;
    let bottom = y0 + h - tb / 2.0;
    ctx.set_line_width(tb);
    ctx.begin_path();
    ctx.move_to(left, y0);
    ctx.line_to(left, y0 + h);
    bowl(ctx, left, top, mid, x0 + w_top - tb / 2.0);
    bowl(ctx, left, mid, bottom, x0 + w - tb / 2.0);
    ctx.stroke();
}

/// A bowl: from the stem at `x` along `y_top`, round the right side at
/// `x_right`, back along `y_bottom`.
fn bowl(ctx: &mut dyn DrawCtx, x: f64, y_top: f64, y_bottom: f64, x_right: f64) {
    let r = (y_bottom - y_top) / 2.0;
    let k = KAPPA * r;
    ctx.move_to(x, y_top);
    ctx.line_to(x_right - r, y_top);
    ctx.cubic_to(
        x_right - r + k,
        y_top,
        x_right,
        y_top + r - k,
        x_right,
        y_top + r,
    );
    ctx.cubic_to(
        x_right,
        y_top + r + k,
        x_right - r + k,
        y_bottom,
        x_right - r,
        y_bottom,
    );
    ctx.line_to(x, y_bottom);
}

/// An italic I: a slanted stem between two bars.
fn italic(ctx: &mut dyn DrawCtx, g: &Grid) {
    let h = (g.n * 0.75).round();
    let top = ((g.n - h) / 2.0).floor();
    let bottom = top + h - g.t;
    let (a1, b1) = ((g.n * 0.20).round(), (g.n * 0.60).round());
    let (a2, b2) = ((g.n * 0.40).round(), (g.n * 0.80).round());
    ctx.begin_path();
    ctx.rect(a2, top, b2 - a2, g.t);
    ctx.rect(a1, bottom, b1 - a1, g.t);
    ctx.fill();
    // The stem runs between the bars' middles, so it joins them with no notch.
    ctx.set_line_width(g.t.max(g.thin()));
    ctx.begin_path();
    ctx.move_to((a1 + b1) / 2.0 + g.t * 0.1, bottom + g.t / 2.0);
    ctx.line_to((a2 + b2) / 2.0 + g.t * 0.1, top + g.t / 2.0);
    ctx.stroke();
}

/// A rectilinear S with its middle bar running on past it as the strike line.
fn strike(ctx: &mut dyn DrawCtx, g: &Grid) {
    let mid = ((g.n - g.t) / 2.0).floor();
    let reach = (g.n * 0.28).round();
    let w = (g.n * 0.5).round();
    let x0 = ((g.n - w) / 2.0).floor();
    let (left, right) = (x0 + g.t / 2.0, x0 + w - g.t / 2.0);
    let (top, middle, bottom) = (
        mid - reach + g.t / 2.0,
        mid + g.t / 2.0,
        mid + reach + g.t / 2.0,
    );
    ctx.set_line_width(g.t);
    ctx.set_line_join(LineJoin::Round);
    ctx.begin_path();
    ctx.move_to(right, top);
    ctx.line_to(left, top);
    ctx.line_to(left, middle);
    ctx.line_to(right, middle);
    ctx.line_to(right, bottom);
    ctx.line_to(left, bottom);
    ctx.move_to(0.0, middle);
    ctx.line_to(g.n, middle);
    ctx.stroke();
}

/// Two chevrons: `<`, `>`, the pair of them around a slash.
fn chevrons(ctx: &mut dyn DrawCtx, g: &Grid, spread: f64, reach: f64, slash: bool) {
    let width = if slash { g.thin() } else { g.thin() * 0.85 };
    let c = g.n / 2.0;
    let (x_in, x_out) = (c - g.n * spread, c - g.n * (spread + 0.26));
    ctx.set_line_width(width);
    ctx.set_line_cap(LineCap::Round);
    ctx.set_line_join(LineJoin::Round);
    ctx.begin_path();
    let arm = g.n * reach;
    ctx.move_to(x_in, c - arm);
    ctx.line_to(x_out, c);
    ctx.line_to(x_in, c + arm);
    ctx.move_to(g.n - x_in, c - arm);
    ctx.line_to(g.n - x_out, c);
    ctx.line_to(g.n - x_in, c + arm);
    if slash {
        ctx.move_to(c + g.n * 0.09, c - g.n * 0.34);
        ctx.line_to(c - g.n * 0.09, c + g.n * 0.34);
    }
    ctx.stroke();
}

/// Inline code: `</>`.
fn code(ctx: &mut dyn DrawCtx, g: &Grid) {
    chevrons(ctx, g, 0.20, 0.27, true);
}

/// A rounded box holding `<` and `>`.
fn code_block(ctx: &mut dyn DrawCtx, g: &Grid) {
    let h = (g.n * 0.88).round();
    let y = ((g.n - h) / 2.0).floor();
    ctx.set_line_width(g.line);
    ctx.begin_path();
    ctx.rounded_rect(
        g.line / 2.0,
        y + g.line / 2.0,
        g.n - g.line,
        h - g.line,
        (g.n * 0.14).round(),
    );
    ctx.stroke();
    chevrons(ctx, g, 0.08, 0.17, false);
}

/// Two interlocked chain links on the diagonal.
fn link(ctx: &mut dyn DrawCtx, g: &Grid) {
    ctx.translate(g.n / 2.0, g.n / 2.0);
    ctx.rotate(-std::f64::consts::FRAC_PI_4);
    let (len, height) = (g.n * 0.56, g.n * 0.36);
    ctx.set_line_width(g.thin());
    ctx.begin_path();
    for centre in [-g.n * 0.2, g.n * 0.2] {
        ctx.rounded_rect(centre - len / 2.0, -height / 2.0, len, height, height / 2.0);
    }
    ctx.stroke();
}

/// A grid: an outline, two rules across and one down.
fn table(ctx: &mut dyn DrawCtx, g: &Grid) {
    let h = (g.n * 0.8).round();
    let y = ((g.n - h) / 2.0).floor();
    let line = g.line;
    let (inner_w, inner_h) = (g.n - 2.0 * line, h - 2.0 * line);
    ctx.set_line_width(line);
    ctx.begin_path();
    ctx.rounded_rect(
        line / 2.0,
        y + line / 2.0,
        g.n - line,
        h - line,
        (g.n * 0.12).round(),
    );
    ctx.stroke();
    let mut rects = Vec::new();
    for k in 1..=2 {
        let at = (k as f64 * inner_h / 3.0 - line / 2.0).round();
        rects.push((line, y + line + at, inner_w, line));
    }
    rects.push((((g.n - line) / 2.0).floor(), y + line, line, inner_h));
    fill_rects(ctx, &rects);
}
