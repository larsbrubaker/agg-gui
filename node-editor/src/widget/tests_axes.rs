//! Tests for `NodeEditor::with_axes` (`axes.rs`): a red x axis and a green
//! y axis through the canvas origin, 1 unit wide across the whole editor at
//! any pan and zoom (MatterCAD's `NodeEditor.OnDraw`), and nothing when off.

use agg_gui::Size;

use super::axes::{axis_segments, AXIS_X_COLOR, AXIS_Y_COLOR};
use super::tests_common::Memory;
use super::*;
use crate::test_recorder::{same, Op, Recorder, Shot};

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};

fn editor(axes: Option<bool>) -> NodeEditor {
    let shared: SharedModel = Arc::new(Mutex::new(Memory::default()));
    let mut editor = NodeEditor::new(shared).with_canvas_grid(false);
    if let Some(axes) = axes {
        editor = editor.with_axes(axes);
    }
    editor.set_bounds(Rect::new(0.0, 0.0, VIEW.width, VIEW.height));
    editor.layout(VIEW);
    editor
}

/// The strokes painted in `color`.
fn strokes_in(editor: &mut NodeEditor, color: agg_gui::Color) -> Vec<Shot> {
    let mut r = Recorder::default();
    editor.paint_canvas(&mut r);
    r.strokes()
        .filter(|s| same(s.color, color))
        .cloned()
        .collect()
}

#[test]
fn the_axes_are_off_by_default() {
    let mut e = editor(None);
    assert!(!e.axes());
    assert!(strokes_in(&mut e, AXIS_X_COLOR).is_empty());
    assert!(strokes_in(&mut e, AXIS_Y_COLOR).is_empty());
}

#[test]
fn the_axes_cross_at_the_canvas_origin_under_pan_and_zoom() {
    let mut e = editor(Some(true));
    assert!(e.axes());
    for (offset, scale) in [
        ([0.0, 0.0], 1.0),
        ([250.0, 180.0], 2.5),
        ([-40.0, 90.0], 0.3),
    ] {
        e.canvas_offset = offset;
        e.canvas_scale = scale;
        // The canvas origin, in editor-local space.
        let [ox, oy] = offset;

        let x_axis = strokes_in(&mut e, AXIS_X_COLOR);
        if (0.0..=VIEW.height).contains(&oy) {
            assert_eq!(x_axis.len(), 1, "{offset:?} {scale}");
            assert_eq!(
                x_axis[0].path,
                vec![Op::MoveTo([0.0, oy]), Op::LineTo([VIEW.width, oy])]
            );
            assert_eq!(x_axis[0].width, 1.0, "the zoom does not widen the axis");
        } else {
            assert!(x_axis.is_empty());
        }

        let y_axis = strokes_in(&mut e, AXIS_Y_COLOR);
        if (0.0..=VIEW.width).contains(&ox) {
            assert_eq!(y_axis.len(), 1, "{offset:?} {scale}");
            assert_eq!(
                y_axis[0].path,
                vec![Op::MoveTo([ox, 0.0]), Op::LineTo([ox, VIEW.height])]
            );
            assert_eq!(y_axis[0].width, 1.0);
            assert_eq!(y_axis[0].cap, agg_gui::LineCap::Butt);
        } else {
            assert!(y_axis.is_empty(), "the y axis is scrolled off: {offset:?}");
        }
    }
}

#[test]
fn an_axis_scrolled_off_the_editor_is_not_drawn() {
    let mut e = editor(Some(true));
    e.canvas_offset = [VIEW.width + 1.0, -1.0];
    assert!(strokes_in(&mut e, AXIS_X_COLOR).is_empty());
    assert!(strokes_in(&mut e, AXIS_Y_COLOR).is_empty());
    // An axis on the editor's edge still draws (MatterCAD's inclusive test).
    let (h, v) = axis_segments(VIEW.width, VIEW.height, [VIEW.width, 0.0]);
    assert_eq!(h, Some([[0.0, 0.0], [VIEW.width, 0.0]]));
    assert_eq!(v, Some([[VIEW.width, 0.0], [VIEW.width, VIEW.height]]));
}

#[test]
fn the_axes_draw_straight_after_the_canvas_background() {
    let mut e = editor(Some(true));
    e.canvas_offset = [100.0, 100.0];
    let mut r = Recorder::default();
    e.paint_canvas(&mut r);
    let first = r.shots.first().expect("the canvas background");
    assert!(first.fill, "the background fill comes first");
    let x_at = r
        .shots
        .iter()
        .position(|s| !s.fill && same(s.color, AXIS_X_COLOR));
    assert_eq!(x_at, Some(1), "the axes come straight after the background");
}
