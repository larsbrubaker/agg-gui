//! Tests for [`TextArea`]'s undo / redo, undo baseline, scroll offset setter
//! and line-end normalisation (`undo.rs`, `scroll.rs`), driven through real
//! `KeyDown` / mouse events where C# drives them through input.

use std::sync::Arc;

use super::*;
use crate::event::{Event, MouseButton};
use crate::geometry::Point;
use crate::widget::Widget;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn area(text: &str) -> TextArea {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut ta = TextArea::new(font).with_text(text);
    ta.layout(Size::new(400.0, 200.0));
    ta
}

fn key(ta: &mut TextArea, key: Key, modifiers: Modifiers) {
    ta.on_event(&Event::KeyDown { key, modifiers });
}

fn type_char(ta: &mut TextArea, c: char) {
    key(ta, Key::Char(c), Modifiers::default());
}

fn ctrl(shift: bool) -> Modifiers {
    Modifiers {
        ctrl: true,
        shift,
        ..Modifiers::default()
    }
}

/// C# `MarkdownEditWidgetTests.UndoReturnsToLoadedMarkdownInsteadOfInitialEmptyState`
/// at the widget level: loading text is not an undo step.
#[test]
fn undo_returns_to_the_baseline_text_not_the_initial_empty_state() {
    let mut ta = area("");
    ta.set_text_as_undo_baseline("abc", 3);
    type_char(&mut ta, '!');
    assert_eq!(ta.text(), "abc!");
    ta.undo();
    ta.undo();
    assert_eq!(ta.text(), "abc");
    ta.undo();
    assert_eq!(ta.text(), "abc");
    assert!(!ta.can_undo());
}

#[test]
fn each_typed_character_is_one_step_and_the_chords_undo_and_redo() {
    let mut ta = area("");
    ta.set_text_as_undo_baseline("", 0);
    for c in "abc".chars() {
        type_char(&mut ta, c);
    }
    key(&mut ta, Key::Char('z'), ctrl(false));
    assert_eq!(ta.text(), "ab");
    key(&mut ta, Key::Char('z'), ctrl(false));
    assert_eq!(ta.text(), "a");
    key(&mut ta, Key::Char('Z'), ctrl(true));
    assert_eq!(ta.text(), "ab");
    key(&mut ta, Key::Char('y'), ctrl(false));
    assert_eq!(ta.text(), "abc");
    assert_eq!(ta.cursor(), 3, "redo puts the caret where the edit left it");
    assert!(!ta.can_redo());
}

#[test]
fn cmd_z_undoes_on_macos() {
    let mut ta = area("x");
    ta.set_cursor_to_end();
    type_char(&mut ta, 'y');
    let cmd = Modifiers {
        meta: true,
        ..Modifiers::default()
    };
    key(&mut ta, Key::Char('z'), cmd);
    assert_eq!(ta.text(), "x");
}

#[test]
fn a_new_edit_after_undo_forgets_the_undone_steps() {
    let mut ta = area("");
    type_char(&mut ta, 'a');
    type_char(&mut ta, 'b');
    ta.undo();
    type_char(&mut ta, 'c');
    ta.redo();
    assert_eq!(ta.text(), "ac");
    ta.undo();
    assert_eq!(ta.text(), "a");
}

#[test]
fn deletes_and_enter_are_undo_steps_and_a_no_op_delete_is_not() {
    let mut ta = area("ab");
    ta.set_cursor_to_start();
    key(&mut ta, Key::Backspace, Modifiers::default());
    assert!(!ta.can_undo(), "backspace at the start changed nothing");
    key(&mut ta, Key::Delete, Modifiers::default());
    key(&mut ta, Key::Enter, Modifiers::default());
    assert_eq!(ta.text(), "\nb");
    ta.undo();
    assert_eq!(ta.text(), "b");
    ta.undo();
    assert_eq!(ta.text(), "ab");
}

/// C#'s `Text` setter: not a step, but the next edit's undo returns to it.
#[test]
fn set_text_is_not_a_step_but_the_next_undo_returns_to_it() {
    let mut ta = area("one");
    ta.set_text("two");
    assert_eq!(ta.cursor(), 0);
    ta.set_cursor_to_end();
    type_char(&mut ta, '!');
    ta.undo();
    assert_eq!(ta.text(), "two");
    ta.undo();
    assert_eq!(ta.text(), "two");
}

/// A change made through the shared edit state is where the next step starts.
#[test]
fn an_external_change_through_the_shared_state_starts_the_next_step() {
    let mut ta = area("");
    type_char(&mut ta, 'a');
    {
        let state = ta.edit_state();
        let mut st = state.borrow_mut();
        st.text = "outside".to_string();
        st.cursor = 7;
        st.anchor = 7;
        st.note_text_change();
    }
    type_char(&mut ta, '!');
    ta.undo();
    assert_eq!(ta.text(), "outside");
}

#[test]
fn baseline_normalises_line_ends_and_moves_the_caret_with_its_character() {
    let mut ta = area("");
    // char index 8 is the 's' of "second" in the raw text.
    ta.set_text_as_undo_baseline("first\r\nsecond\rthird", 7);
    assert_eq!(ta.text(), "first\nsecond\nthird");
    assert_eq!(ta.char_index_to_insert_before(), 6);
    assert_eq!(ta.selection(), None);
}

#[test]
fn normalize_line_endings_matches_csharp_text_line_endings() {
    assert_eq!(normalize_line_endings("", 3), (String::new(), 0));
    assert_eq!(normalize_line_endings("a\r\nb", 4), ("a\nb".to_string(), 3));
    assert_eq!(normalize_line_endings("a\r\nb", 2), ("a\nb".to_string(), 2));
    assert_eq!(normalize_line_endings("a\r\rb", 3), ("a\n\nb".to_string(), 3));
}

#[test]
fn pasted_crlf_text_is_normalised() {
    let mut ta = area("");
    ta.insert_str("x\r\ny\rz");
    assert_eq!(ta.text(), "x\ny\nz");
}

/// C# `MarkdownEditWidgetTests.TextEditWidgetClickingVisualPositionInCrLfTextUsesCorrectSourceIndex`
/// at the widget level: a click where "## Designing" visually ends, in text
/// loaded with CRLF line ends, inserts there.
#[test]
fn clicking_visual_position_in_crlf_text_uses_correct_source_index() {
    let mut ta = area("first\r\nsecond\r\n## Designing\r\nlast");
    let normalized = "first\nsecond\n## Designing\nlast";
    let visual = area(normalized);
    let index = normalized.find("## Designing").unwrap() + "## Designing".len();
    let p = visual.pos_for_cursor(index);
    let click = Point::new(p.x + 1.0, p.y + visual.cached_line_h * 0.5);

    ta.on_event(&Event::FocusGained);
    ta.set_cursor_to_start();
    ta.on_event(&Event::MouseDown {
        pos: click,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    ta.on_event(&Event::MouseUp {
        pos: click,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    type_char(&mut ta, '!');
    assert_eq!(ta.text(), "first\nsecond\n## Designing!\nlast");
}

#[test]
fn scroll_offset_is_settable_clamped_and_resettable() {
    let text: String = (0..60).map(|i| format!("line {i}\n")).collect();
    let mut ta = area(&text);
    ta.set_scroll_offset(40.0);
    assert_eq!(ta.scroll_offset(), 40.0);
    ta.set_scroll_offset(1.0e9);
    assert_eq!(ta.scroll_offset(), ta.max_scroll_y());
    ta.set_scroll_offset(-5.0);
    assert_eq!(ta.scroll_offset(), 0.0);
}
