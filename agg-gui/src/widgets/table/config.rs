//! Configuration types for the `Table` widget.
//!
//! Defines column sizing, row descriptions, cell/header info structs, and
//! the painter callback type aliases shared by the builder and widget impl.

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;
use crate::text::Font;
use crate::theme::Visuals;

// ── Configuration types ─────────────────────────────────────────────────────

/// How a column derives its width.
#[derive(Clone, Copy, Debug)]
pub enum ColumnSize {
    Auto(f64),
    Exact(f64),
    Remainder { at_least: f64, clip: bool },
}

#[derive(Clone, Copy, Debug)]
pub struct TableColumn {
    pub size: ColumnSize,
    pub resizable: bool,
}

impl TableColumn {
    pub fn auto(initial: f64) -> Self {
        Self {
            size: ColumnSize::Auto(initial),
            resizable: false,
        }
    }
    pub fn exact(w: f64) -> Self {
        Self {
            size: ColumnSize::Exact(w),
            resizable: false,
        }
    }
    pub fn remainder() -> Self {
        Self {
            size: ColumnSize::Remainder {
                at_least: 16.0,
                clip: false,
            },
            resizable: false,
        }
    }
    pub fn at_least(mut self, v: f64) -> Self {
        if let ColumnSize::Remainder { ref mut at_least, .. } = self.size {
            *at_least = v;
        }
        self
    }
    pub fn clip(mut self, on: bool) -> Self {
        if let ColumnSize::Remainder { ref mut clip, .. } = self.size {
            *clip = on;
        }
        self
    }
    pub fn resizable(mut self, on: bool) -> Self {
        self.resizable = on;
        self
    }
}

/// Minimum width any column can be resized to.
pub const MIN_COL_W: f64 = 16.0;
/// Pixel half-width of the resize hit zone around a column's right edge.
pub const RESIZE_HIT_HALF: f64 = 4.0;

/// Distribute `total_w` across `columns` according to their sizing modes.
///
/// `overrides[i] = Some(w)` pins column `i` to the user-resized width and
/// removes it from the auto/remainder distribution.  `Remainder` columns
/// share whatever space is left after fixed + overridden columns, never
/// going below their `at_least`.
pub fn distribute_widths(
    columns: &[TableColumn],
    total_w: f64,
    overrides: &[Option<f64>],
) -> Vec<f64> {
    let n = columns.len();
    let mut out = vec![0.0_f64; n];
    let mut remainder_indices: Vec<usize> = Vec::new();
    let mut remainder_min = 0.0_f64;
    let mut fixed_total = 0.0_f64;
    for (i, c) in columns.iter().enumerate() {
        if let Some(w) = overrides.get(i).copied().flatten() {
            out[i] = w.max(MIN_COL_W);
            fixed_total += out[i];
            continue;
        }
        match c.size {
            ColumnSize::Auto(w) | ColumnSize::Exact(w) => {
                out[i] = w;
                fixed_total += w;
            }
            ColumnSize::Remainder { at_least, .. } => {
                remainder_indices.push(i);
                remainder_min += at_least;
            }
        }
    }
    let leftover = (total_w - fixed_total).max(remainder_min);
    if !remainder_indices.is_empty() {
        let each = leftover / remainder_indices.len() as f64;
        for &i in &remainder_indices {
            let at_least = match columns[i].size {
                ColumnSize::Remainder { at_least, .. } => at_least,
                _ => 0.0,
            };
            out[i] = each.max(at_least);
        }
    }
    out
}

/// Row-set description.
#[derive(Clone, Debug)]
pub enum TableRows {
    Homogeneous { count: usize, height: f64 },
    Heterogeneous { heights: Vec<f64> },
}

impl TableRows {
    pub fn count(&self) -> usize {
        match self {
            TableRows::Homogeneous { count, .. } => *count,
            TableRows::Heterogeneous { heights } => heights.len(),
        }
    }
    pub fn height_at(&self, i: usize) -> f64 {
        match self {
            TableRows::Homogeneous { height, .. } => *height,
            TableRows::Heterogeneous { heights } => heights.get(i).copied().unwrap_or(0.0),
        }
    }
    pub fn total_height(&self) -> f64 {
        match self {
            TableRows::Homogeneous { count, height } => *count as f64 * *height,
            TableRows::Heterogeneous { heights } => heights.iter().copied().sum(),
        }
    }
    pub fn top_down_y_at(&self, i: usize) -> f64 {
        match self {
            TableRows::Homogeneous { height, .. } => i as f64 * *height,
            TableRows::Heterogeneous { heights } => {
                let take = i.min(heights.len());
                heights[..take].iter().copied().sum()
            }
        }
    }
}

// ── Painter callback shapes ─────────────────────────────────────────────────

pub struct CellInfo<'a> {
    pub row: usize,
    pub col: usize,
    /// Cell rect in widget-local Y-up coordinates of the table body.
    /// The table has clipped to this rect before invoking the painter.
    pub rect: Rect,
    pub selected: bool,
    pub visuals: &'a Visuals,
    pub font: &'a Arc<Font>,
}

pub struct HeaderInfo<'a> {
    pub col: usize,
    pub rect: Rect,
    pub visuals: &'a Visuals,
    pub font: &'a Arc<Font>,
}

pub type CellPainter = Box<dyn FnMut(&CellInfo, &mut dyn DrawCtx)>;
pub type HeaderPainter = Box<dyn FnMut(&HeaderInfo, &mut dyn DrawCtx)>;
/// Optional callback for header clicks. Receives column index and click
/// position relative to the header *cell* rect (Y-up).
pub type HeaderClick = Box<dyn FnMut(usize, f64, f64) -> crate::event::EventResult>;
/// Per-row predicate (e.g. for overlines).
pub type RowPredicate = Box<dyn Fn(usize) -> bool>;
/// Callback returning the live row spec.  Called on every layout pass when
/// set, so external state changes (e.g. switching demo modes or
/// resizing the dataset) flow into the table without an observer widget.
pub type RowsProvider = Box<dyn Fn() -> TableRows>;
