//! Tests for updating a live [`MarkdownView`] in place
//! ([`set_markdown`](MarkdownView::set_markdown)) and for
//! [`has_selection`](MarkdownView::has_selection): the new text renders, a
//! selection made while text streams in survives, a selection the new text no
//! longer covers is clamped onto it (never inside a character), and code-block
//! scroll offsets carry over.

use std::sync::Arc;

use crate::event::{Event, Modifiers, MouseButton};
use crate::geometry::{Point, Size};
use crate::text::Font;
use crate::widget::Widget;

use super::MarkdownView;

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn view(markdown: &str) -> MarkdownView {
    crate::widget::set_current_viewport(Size::new(480.0, 320.0));
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
    let mut view = MarkdownView::new(markdown, font);
    view.layout(Size::new(480.0, 2000.0));
    view
}

fn relayout(view: &mut MarkdownView) {
    view.layout(Size::new(480.0, 2000.0));
}

/// Drag with the left button from `from` to `to` (widget-local, Y-up).
fn drag(view: &mut MarkdownView, from: Point, to: Point) {
    let modifiers = Modifiers::default();
    let button = MouseButton::Left;
    view.on_event(&Event::MouseDown {
        pos: from,
        button,
        modifiers,
    });
    view.on_event(&Event::MouseMove { pos: to });
    view.on_event(&Event::MouseUp {
        pos: to,
        button,
        modifiers,
    });
}

#[test]
fn rust_only_set_markdown_renders_the_new_text_in_place() {
    let mut v = view("Hello");
    v.set_markdown("Hello **world**");
    assert_eq!(v.markdown(), "Hello **world**");
    relayout(&mut v);
    // The paragraph ends in a line-break gap.
    assert_eq!(v.selectable_text, "Hello world\n");
}

#[test]
fn rust_only_has_selection_follows_a_drag_across_the_words() {
    let mut v = view("Hello streaming world");
    assert!(!v.has_selection(), "a fresh view has no selection");
    let fragment = v.selectable_fragments[0].clone();
    let y = fragment.y + fragment.height * 0.5;
    drag(
        &mut v,
        Point::new(fragment.text_x + 1.0, y),
        Point::new(fragment.text_x + fragment.width * 0.5, y),
    );
    assert!(v.has_selection(), "dragging across the words selects them");
    v.clear_selection();
    assert!(!v.has_selection());
}

#[test]
fn rust_only_set_markdown_keeps_a_selection_while_text_streams_in() {
    let mut v = view("Hello");
    // As a drag across the word selects it.
    v.selection_anchor = Some(0);
    v.selection_cursor = Some(5);
    for streamed in [
        "Hello wor",
        "Hello world, and more",
        "Hello world, and more\n\nNext",
    ] {
        v.set_markdown(streamed);
        relayout(&mut v);
        assert!(v.has_selection(), "selection survives {streamed:?}");
        assert_eq!(v.selection_range(), Some(0..5), "after {streamed:?}");
    }
}

#[test]
fn rust_only_set_markdown_clamps_a_selection_past_the_new_end() {
    let mut v = view("Hello world");
    v.select_all_text();
    v.set_markdown("Hi");
    relayout(&mut v);
    assert_eq!(v.selectable_text, "Hi\n");
    assert_eq!(v.selection_range(), Some(0..3));
    let (markdown, _) = v.copy_payloads(v.selection_range().expect("selection"));
    assert_eq!(markdown.trim_end(), "Hi");
}

#[test]
fn rust_only_set_markdown_never_leaves_a_selection_inside_a_character() {
    let mut v = view("aaa");
    v.selection_anchor = Some(0);
    v.selection_cursor = Some(2);
    // Byte 2 now falls inside 'é' (bytes 1..3): the selection must snap back
    // to a character boundary rather than panic when copied.
    v.set_markdown("aé");
    relayout(&mut v);
    assert_eq!(v.selection_range(), Some(0..1));
    let (markdown, _) = v.copy_payloads(v.selection_range().expect("selection"));
    assert_eq!(markdown, "a");
}

#[test]
fn rust_only_set_markdown_keeps_code_block_scroll() {
    let long = "x".repeat(400);
    let mut v = view(&format!("```\n{long}\n```"));
    let (_, viewport, content) = v.block_metrics(0).expect("a scrollable code block");
    assert!(v.scroll_block_to(0, 120.0, viewport, content));
    let offset = v.block_scroll_offset(0);
    assert!(offset > 0.0);
    v.set_markdown(&format!("```\n{long}\n```\n\nMore text"));
    relayout(&mut v);
    assert_eq!(v.block_scroll_offset(0), offset);
}

#[test]
fn rust_only_set_markdown_with_the_same_text_keeps_everything() {
    let mut v = view("Same");
    v.select_all_text();
    let selected = v.selection_range();
    assert!(selected.is_some());
    v.set_markdown("Same");
    relayout(&mut v);
    assert_eq!(v.markdown(), "Same");
    assert_eq!(v.selection_range(), selected);
}
