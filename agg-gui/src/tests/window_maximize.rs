//! Tests for canvas-level floating-window maximize and persistence behavior.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::geometry::{Rect, Size};
use crate::text::Font;
use crate::widgets::window::Window;
use crate::{Label, Widget};

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

#[test]
fn test_window_restores_canvas_maximized_state() {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let pos_cell = Rc::new(Cell::new(Rect::default()));
    let max_cell = Rc::new(Cell::new(true));
    let saved_normal = Rect::new(50.0, 80.0, 400.0, 220.0);
    let content: Box<dyn crate::widget::Widget> =
        Box::new(Label::new("content", Arc::clone(&font)));
    let mut win = Window::new("Restored Max", Arc::clone(&font), content)
        .with_bounds(saved_normal)
        .with_position_cell(Rc::clone(&pos_cell))
        .with_maximized_cell(Rc::clone(&max_cell));

    let _ = <Window as Widget>::layout(&mut win, Size::new(640.0, 480.0));

    assert_eq!(
        win.bounds(),
        Rect::new(0.0, 0.0, 640.0, 480.0),
        "restored maximized windows should fill the current canvas"
    );
    assert_eq!(
        pos_cell.get(),
        saved_normal,
        "autosave should keep the pre-maximize bounds, not the canvas-sized bounds"
    );
    assert!(
        max_cell.get(),
        "autosave should preserve the separate maximized flag"
    );
}
