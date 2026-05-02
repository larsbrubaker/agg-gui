//! Shared live state between `Table`, `TableBody`, and `TableBuilder`.
//!
//! `TableState` holds all `Rc`-wrapped mutable cells so the three components
//! can observe and update them without the borrow checker blocking cross-part
//! access.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::geometry::Rect;

use super::config::{CellPainter, RowPredicate, RowsProvider, TableRows};

// ── Live state shared between Table and its body ────────────────────────────

#[derive(Clone)]
pub(crate) struct TableState {
    pub(crate) rows: Rc<RefCell<TableRows>>,
    pub(crate) rows_provider: Rc<RefCell<Option<RowsProvider>>>,
    /// Total natural content width — sum of column widths after each layout.
    /// Used by the header paint path to clip and translate horizontally.
    pub(crate) content_w: Rc<Cell<f64>>,
    /// Horizontal scroll offset shared with the inner ScrollView so the
    /// header can stay aligned with the scrolled body.
    pub(crate) h_offset: Rc<Cell<f64>>,
    pub(crate) resizable: Rc<Cell<bool>>,
    pub(crate) striped: Rc<Cell<bool>>,
    pub(crate) overline_pred: Rc<RefCell<Option<RowPredicate>>>,
    pub(crate) sense_click: Rc<Cell<bool>>,
    pub(crate) selection_pred: Rc<RefCell<Option<RowPredicate>>>,
    pub(crate) widths: Rc<RefCell<Vec<f64>>>,
    pub(crate) column_overrides: Rc<RefCell<Vec<Option<f64>>>>,
    pub(crate) viewport_cell: Rc<Cell<Rect>>,
    pub(crate) scroll_offset: Rc<Cell<f64>>,
    pub(crate) scroll_to_row: Rc<Cell<Option<usize>>>,
    pub(crate) cell_painter: Rc<RefCell<Option<CellPainter>>>,
    pub(crate) on_row_click: Rc<RefCell<Option<Box<dyn FnMut(usize, usize)>>>>,
    /// Row currently under the mouse, painted with a subtle highlight.
    /// `None` when the cursor is outside the body.  Tracked here rather
    /// than in the body widget so external code can observe it.
    pub(crate) hovered_row: Rc<Cell<Option<usize>>>,
}

impl TableState {
    pub(crate) fn defaults() -> Self {
        Self {
            rows: Rc::new(RefCell::new(TableRows::Homogeneous {
                count: 0,
                height: 18.0,
            })),
            rows_provider: Rc::new(RefCell::new(None)),
            content_w: Rc::new(Cell::new(0.0)),
            h_offset: Rc::new(Cell::new(0.0)),
            resizable: Rc::new(Cell::new(true)),
            striped: Rc::new(Cell::new(false)),
            overline_pred: Rc::new(RefCell::new(None)),
            sense_click: Rc::new(Cell::new(false)),
            selection_pred: Rc::new(RefCell::new(None)),
            widths: Rc::new(RefCell::new(Vec::new())),
            column_overrides: Rc::new(RefCell::new(Vec::new())),
            viewport_cell: Rc::new(Cell::new(Rect::default())),
            scroll_offset: Rc::new(Cell::new(0.0)),
            scroll_to_row: Rc::new(Cell::new(None)),
            cell_painter: Rc::new(RefCell::new(None)),
            on_row_click: Rc::new(RefCell::new(None)),
            hovered_row: Rc::new(Cell::new(None)),
        }
    }
}
