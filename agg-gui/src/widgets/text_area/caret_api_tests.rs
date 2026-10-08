//! Unit tests for the C#-shaped caret API in `caret_api.rs` (the ported
//! `TextEditTests.MultiLineTests` in agg-gui-automation drives it through
//! real input; these pin the pieces it cannot see directly).

use std::sync::Arc;

use super::*;
use crate::widget::Widget;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn area(text: &str) -> TextArea {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut ta = TextArea::new(font)
        .with_font_size(16.0)
        .with_padding(0.0)
        .with_line_spacing(1.0)
        .with_text(text);
    ta.layout(Size::new(400.0, 400.0));
    ta
}

#[test]
fn line_spacing_sets_the_line_advance() {
    let ta = area("a\nb\nc");
    assert_eq!(ta.content_height(), 48.0);
}

#[test]
fn index_setters_clamp_out_of_range_values() {
    let mut ta = area("tést");
    ta.set_char_index_to_insert_before(-3);
    ta.set_selection_index_to_start_before(99);
    assert_eq!(ta.char_index_to_insert_before(), 0);
    assert_eq!(ta.selection_index_to_start_before(), 4);
    assert_eq!(ta.selected_text(), "tést");
    ta.set_char_index_to_insert_before(2);
    assert_eq!(ta.selected_text(), "st");
}

#[test]
fn insert_bar_position_counts_lines_down_from_zero() {
    let mut ta = area("ab\ncd");
    ta.set_char_index_to_insert_before(4);
    ta.set_selection_index_to_start_before(4);
    let at = ta.insert_bar_position();
    assert_eq!(at.y, -16.0);
    assert!(at.x > 0.0);
}
