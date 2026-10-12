//! The geometry of a `MarkdownView` table's grid, from MarkdigAgg's
//! `Tables/AggTable.cs` (`ReserveGridLineSpace`, `MeasureGrid`,
//! `OnDrawBackground`, `OnDraw`).
//!
//! C# opens one grid line of whole device pixels before every cell and row
//! and after the last ones, so the lines drawn into those gaps never touch
//! cell content; [`TableGrid`] is that layout in the view's logical units
//! (`layout.rs` sizes a table with it, and `paint.rs`, `selection.rs`,
//! `cell_links.rs` and `introspect.rs` place cells with it).  At paint time
//! [`SnappedGrid`] moves every line onto whole device pixels under the
//! current draw transform (C#'s `GridPixels.Snap`, here also through the
//! transform's scale, because agg-gui widgets draw in logical units under a
//! device-scale transform where C# laid out in device pixels), fills the
//! zebra stripes between the lines and draws the lines once each: the rules
//! across the grid, the verticals between the rules, so a crossing is never
//! painted twice in the translucent grid colour.

use agg_rust::trans_affine::TransAffine;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;

/// One grid line's thickness in logical units at `device_scale`: a whole
/// number of device pixels (`grid_pixels::line_thickness`) divided back
/// down, as C#'s `GridLineThickness / DeviceScale`.
pub(super) fn line_for_scale(device_scale: f64) -> f64 {
    let scale = device_scale.max(f64::MIN_POSITIVE);
    f64::from(crate::grid_pixels::line_thickness(scale)) / scale
}

/// `value` (logical) rounded up to whole device pixels at `device_scale`,
/// as C# `Math.Ceiling` sized cells in device pixels.  Whole-pixel cells
/// keep every snapped column exactly its laid-out width.
pub(super) fn ceil_to_device_pixels(value: f64, device_scale: f64) -> f64 {
    let scale = device_scale.max(f64::MIN_POSITIVE);
    (value * scale).ceil() / scale
}

/// A laid-out table's cells between their reserved grid lines.
#[derive(Clone, Copy)]
pub(super) struct TableGrid<'a> {
    pub col_widths: &'a [f64],
    pub row_heights: &'a [f64],
    /// One grid line, in logical units.
    pub line: f64,
}

impl TableGrid<'_> {
    /// The grid's whole width: every column and every vertical line.
    pub fn width(&self) -> f64 {
        self.col_widths.iter().sum::<f64>() + self.line * (self.col_widths.len() + 1) as f64
    }

    /// The grid's whole height: every row and every rule.
    pub fn height(&self) -> f64 {
        self.row_heights.iter().sum::<f64>() + self.line * (self.row_heights.len() + 1) as f64
    }

    /// Column `ci`'s cell left edge, from the grid's left edge (after the
    /// line before it).
    pub fn cell_left(&self, ci: usize) -> f64 {
        self.line * (ci + 1) as f64 + self.col_widths.iter().take(ci).sum::<f64>()
    }

    /// Row `ri`'s bottom edge (rows top first), from the grid's bottom edge.
    pub fn row_bottom(&self, ri: usize) -> f64 {
        self.height()
            - self.line * (ri + 1) as f64
            - self.row_heights.iter().take(ri + 1).sum::<f64>()
    }

    /// Row `ri`'s top edge (below the rule above it), from the grid's
    /// bottom edge.
    pub fn row_top(&self, ri: usize) -> f64 {
        self.row_bottom(ri) + self.row_heights.get(ri).copied().unwrap_or(0.0)
    }
}

/// The grid snapped for one paint, in drawing coordinates: each vertical
/// line's left edge (left to right), each rule's bottom edge (top to
/// bottom), and the line thickness and the device pixel per axis, both in
/// drawing units.
pub(super) struct SnappedGrid {
    pub lefts: Vec<f64>,
    pub bottoms: Vec<f64>,
    pub thick_x: f64,
    pub thick_y: f64,
}

/// `value` moved to the nearest whole device pixel under a transform with
/// `scale` and `translation` on that axis, in drawing coordinates
/// (`floor(v + 0.5)` like `grid_pixels::snap`, which this extends with the
/// scale).  Assumes no rotation or shear.
fn snap(value: f64, scale: f64, translation: f64) -> f64 {
    ((value * scale + translation + 0.5).floor() - translation) / scale
}

impl SnappedGrid {
    /// Snaps `grid`, whose bottom-left corner is at `left`, `bottom` in
    /// drawing coordinates, under `transform` (C# `MeasureGrid`).  None for
    /// a table with no rows or columns, which has no grid.
    pub fn new(
        grid: &TableGrid<'_>,
        left: f64,
        bottom: f64,
        transform: &TransAffine,
        device_scale: f64,
    ) -> Option<Self> {
        let cols = grid.col_widths.len();
        let rows = grid.row_heights.len();
        if cols == 0 || rows == 0 {
            return None;
        }
        let (sx, sy) = (
            transform.sx.max(f64::MIN_POSITIVE),
            transform.sy.max(f64::MIN_POSITIVE),
        );
        let pixels = f64::from(crate::grid_pixels::line_thickness(device_scale));
        let (thick_x, thick_y) = (pixels / sx, pixels / sy);
        let mut lefts: Vec<f64> = (0..cols)
            .map(|ci| snap(left + grid.cell_left(ci), sx, transform.tx) - thick_x)
            .collect();
        let last_right = left + grid.cell_left(cols - 1) + grid.col_widths[cols - 1];
        lefts.push(snap(last_right, sx, transform.tx));
        let mut bottoms: Vec<f64> = (0..rows)
            .map(|ri| snap(bottom + grid.row_top(ri), sy, transform.ty))
            .collect();
        bottoms.push(snap(bottom + grid.row_bottom(rows - 1), sy, transform.ty) - thick_y);
        Some(Self {
            lefts,
            bottoms,
            thick_x,
            thick_y,
        })
    }

    /// Fills each row's stripe between the grid lines, so a stripe never
    /// tints a line (C# `OnDrawBackground`).
    pub fn paint_stripes(&self, ctx: &mut dyn DrawCtx, stripe: impl Fn(usize) -> Color) {
        for r in 0..self.bottoms.len() - 1 {
            let color = stripe(r);
            if color.a <= 0.0 {
                continue;
            }
            ctx.set_fill_color(color);
            ctx.begin_path();
            let y0 = self.bottoms[r + 1] + self.thick_y;
            let y1 = self.bottoms[r];
            for c in 0..self.lefts.len() - 1 {
                let x0 = self.lefts[c] + self.thick_x;
                ctx.rect(x0, y0, self.lefts[c + 1] - x0, y1 - y0);
            }
            ctx.fill();
        }
    }

    /// Draws the rules across the grid and the verticals between them
    /// (C# `OnDraw`).  Every rectangle is its own fill so the translucent
    /// grid colour never doubles up where two would overlap.
    pub fn paint_lines(&self, ctx: &mut dyn DrawCtx, color: Color) {
        ctx.set_fill_color(color);
        let grid_left = self.lefts[0];
        let grid_right = self.lefts[self.lefts.len() - 1] + self.thick_x;
        for &b in &self.bottoms {
            ctx.begin_path();
            ctx.rect(grid_left, b, grid_right - grid_left, self.thick_y);
            ctx.fill();
        }
        for &l in &self.lefts {
            for r in 0..self.bottoms.len() - 1 {
                let y0 = self.bottoms[r + 1] + self.thick_y;
                ctx.begin_path();
                ctx.rect(l, y0, self.thick_x, self.bottoms[r] - y0);
                ctx.fill();
            }
        }
    }
}
