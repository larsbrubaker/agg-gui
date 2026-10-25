//! Rust-only: the mouse wheel over a `RichMarkdownEditWidget`.
//!
//! A wheel event carries notches (`Event::MouseWheel`), and every scrolling
//! widget moves `agg_gui::wheel::PIXELS_PER_NOTCH` px per notch, so one click
//! of a mouse wheel moves the editor's text as far as it moves a `ScrollView`,
//! and a trackpad's fractional notches move it as far as the fingers moved.

use agg_gui::event::{Event, EventResult, Modifiers};
use agg_gui::geometry::Point;
use agg_gui::wheel::PIXELS_PER_NOTCH;
use agg_gui::Widget;

use super::rich_markdown_edit_widget_tests::Container;

fn long_document() -> String {
    (0..80)
        .map(|i| format!("Paragraph number {i} with a few words.\n\n"))
        .collect()
}

fn wheel(editor: &mut dyn Widget, notches: f64) -> EventResult {
    editor.on_event(&Event::MouseWheel {
        pos: Point::new(200.0, 150.0),
        delta_y: notches,
        delta_x: 0.0,
        modifiers: Modifiers::default(),
    })
}

#[test]
fn one_wheel_notch_scrolls_the_editor_as_far_as_a_scroll_view() {
    let mut c = Container::new(&long_document(), 400.0, 300.0);
    assert_eq!(c.editor.scroll_offset_from_top(), 0.0);

    // Wheel toward the content below: one notch down.
    assert_eq!(wheel(&mut c.editor, -1.0), EventResult::Consumed);
    assert_eq!(c.editor.scroll_offset_from_top(), PIXELS_PER_NOTCH);

    // A trackpad's fractional notches add up to the travel they stand for.
    for _ in 0..4 {
        wheel(&mut c.editor, -0.25);
    }
    assert_eq!(c.editor.scroll_offset_from_top(), 2.0 * PIXELS_PER_NOTCH);

    // Forward toward the content above.
    assert_eq!(wheel(&mut c.editor, 1.0), EventResult::Consumed);
    assert_eq!(c.editor.scroll_offset_from_top(), PIXELS_PER_NOTCH);
}

#[test]
fn a_wheel_at_the_top_is_left_for_an_ancestor() {
    let mut c = Container::new(&long_document(), 400.0, 300.0);
    assert_eq!(wheel(&mut c.editor, 1.0), EventResult::Ignored);
    assert_eq!(c.editor.scroll_offset_from_top(), 0.0);
}
