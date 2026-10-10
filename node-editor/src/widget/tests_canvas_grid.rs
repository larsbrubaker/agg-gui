//! Tests for `NodeEditor::with_canvas_grid` (`presentation.rs`): the grid
//! backdrop `paint.rs` draws under the noodles, on by default and off for a
//! host that wants a plain canvas (MatterCAD's node editor draws none).

use agg_gui::Size;

use super::tests_common::Memory;
use super::*;
use crate::test_recorder::{Op, Recorder};

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};

fn editor(grid: Option<bool>) -> NodeEditor {
    let shared: SharedModel = Arc::new(Mutex::new(Memory::default()));
    let mut editor = NodeEditor::new(shared);
    if let Some(grid) = grid {
        editor = editor.with_canvas_grid(grid);
    }
    editor.set_bounds(Rect::new(0.0, 0.0, VIEW.width, VIEW.height));
    editor.layout(VIEW);
    editor
}

/// Straight two-point strokes: the grid's lines.
fn grid_lines(editor: &mut NodeEditor) -> usize {
    let mut r = Recorder::default();
    editor.paint_canvas(&mut r);
    r.strokes()
        .filter(|s| s.path.len() == 2 && matches!(s.path.get(1), Some(Op::LineTo(..))))
        .count()
}

#[test]
fn the_canvas_grid_is_drawn_by_default() {
    let mut e = editor(None);
    assert!(e.canvas_grid());
    // 800 x 600 at 40-unit cells: 21 columns and 16 rows.
    assert_eq!(grid_lines(&mut e), 21 + 16);
}

#[test]
fn a_canvas_without_its_grid_draws_no_grid_line() {
    let mut e = editor(Some(false));
    assert!(!e.canvas_grid());
    assert_eq!(grid_lines(&mut e), 0);
}
