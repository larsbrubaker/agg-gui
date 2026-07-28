//! Selection + code-editor keyboard tests for [`TextArea`], kept separate from
//! `tests.rs` (which is near the 800-line cap) but exercising the same
//! production widget.
//!
//! Covers the pointer-selection funnels (`begin_pointer_selection` /
//! `extend_selection_drag`) that `widget_impl` drives on mouse down and drag
//! (plus a real `MouseDown` event path so multi-click counting is covered end
//! to end), and the standard keyboard set routed through real `KeyDown`
//! events: word-wise caret movement/selection (Ctrl/Alt+arrows, +Shift),
//! word-wise deletion (Ctrl/Alt+Backspace/Delete, selection-first), and
//! Tab/Shift+Tab line indent/outdent.

use std::sync::Arc;

use super::*;
use crate::event::{Event, MouseButton};
use crate::geometry::Point;
use crate::widget::Widget;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn laid_out(text: &str, w: f64, h: f64) -> TextArea {
    let mut ta = TextArea::new(font()).with_text(text);
    ta.layout(Size::new(w, h));
    ta
}

/// A double-click (clicks == 2) selects the word under the byte offset.
#[test]
fn double_click_selects_word() {
    let mut ta = laid_out("hello world", 400.0, 120.0);
    // Offset 8 sits inside "world" (bytes 6..11).
    ta.begin_pointer_selection(8, 2, false);
    assert_eq!(ta.selection(), Some((6, 11)));
    assert_eq!(ta.selected_text(), "world");
}

/// Double-click on a punctuation run selects just that run, not the words.
#[test]
fn double_click_stops_at_punctuation() {
    let mut ta = laid_out("foo.bar", 400.0, 120.0);
    ta.begin_pointer_selection(1, 2, false);
    assert_eq!(ta.selected_text(), "foo");
    ta.begin_pointer_selection(5, 2, false);
    assert_eq!(ta.selected_text(), "bar");
}

/// A triple-click (clicks == 3) selects the whole logical line (paragraph),
/// even though the paragraph wraps across several visual lines.
#[test]
fn triple_click_selects_logical_line() {
    let text = "first line\nsecond line here\nthird";
    let mut ta = laid_out(text, 400.0, 200.0);
    // Offset 15 is inside "second line here" (the middle paragraph).
    ta.begin_pointer_selection(15, 3, false);
    assert_eq!(ta.selected_text(), "second line here");
}

/// Dragging after a double-click extends the selection by whole words.
#[test]
fn word_drag_extends_by_whole_words() {
    let text = "alpha beta gamma";
    let mut ta = laid_out(text, 400.0, 120.0);
    // Double-click "beta" (bytes 6..10).
    ta.begin_pointer_selection(7, 2, false);
    assert_eq!(ta.selected_text(), "beta");
    // Drag right into "gamma": selection should snap to the end of "gamma".
    ta.extend_selection_drag(13);
    assert_eq!(ta.selected_text(), "beta gamma");
    // Drag left into "alpha": selection should snap to the start of "alpha".
    ta.extend_selection_drag(2);
    assert_eq!(ta.selected_text(), "alpha beta");
}

/// A real double `MouseDown` at the same point selects the word, proving the
/// multi-click counter is wired through the event handler.
#[test]
fn double_mouse_down_event_selects_word() {
    let mut ta = laid_out("hello world", 400.0, 120.0);
    ta.on_event(&Event::FocusGained);
    // Local point at the left edge of byte 8, nudged right so hit-testing lands
    // squarely inside "world".
    let p = ta.pos_for_cursor(8);
    let click = Point::new(p.x + 1.0, p.y + 1.0);
    let down = Event::MouseDown {
        pos: click,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };
    ta.on_event(&down);
    ta.on_event(&Event::MouseUp {
        pos: click,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    ta.on_event(&down);
    assert_eq!(ta.selected_text(), "world");
}

// ── Text-rendering pipeline: LCD routing + baseline snapping ────────────────

/// The editor must follow the System LCD setting the same way `Label` /
/// `TextField` do: an `LcdCoverage` backbuffer when LCD is on, a grayscale
/// `Rgba` backbuffer when it is off. Toggling the global flag changes the
/// render-path decision on the next frame.
#[test]
fn backbuffer_mode_follows_lcd_setting() {
    use crate::widget::BackbufferMode;
    // Standard density so the high-scale hard cap in `lcd_enabled` doesn't mask
    // the explicit override under test.
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);

    let ta = laid_out("hello", 400.0, 120.0);

    crate::font_settings::set_lcd_enabled(true);
    assert_eq!(ta.backbuffer_mode(), BackbufferMode::LcdCoverage);

    crate::font_settings::set_lcd_enabled(false);
    assert_eq!(ta.backbuffer_mode(), BackbufferMode::Rgba);

    crate::font_settings::clear_lcd_enabled_override();
}

/// With hinting on, every visual line's baseline lands on an integer pixel row
/// at scale 1 — the same crisp-vertical treatment `Label` gets — so multi-line
/// text does not blur across two rows.
#[test]
fn line_baselines_snap_to_pixel_grid_when_hinting_on() {
    // Font size chosen so the raw (unsnapped) baseline is fractional, proving
    // the snap is doing real work rather than coincidentally landing integer.
    let mut ta = TextArea::new(font())
        .with_text("line one\nline two\nline three")
        .with_font_size(13.0);
    ta.layout(Size::new(400.0, 200.0));

    crate::font_settings::set_hinting_enabled(true);
    for i in 0..ta.visual_line_count() {
        let y = ta.line_baseline_y(i);
        assert_eq!(
            y.fract(),
            0.0,
            "line {i} baseline {y} not on the pixel grid"
        );
    }
    crate::font_settings::set_hinting_enabled(false);
}

// ── Code-editor keyboard set ────────────────────────────────────────────────

fn key_mods(ta: &mut TextArea, k: Key, mods: Modifiers) {
    ta.on_event(&Event::KeyDown {
        key: k,
        modifiers: mods,
    });
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Default::default()
    }
}

fn ctrl_shift() -> Modifiers {
    Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    }
}

fn shift_mod() -> Modifiers {
    Modifiers {
        shift: true,
        ..Default::default()
    }
}

/// Place the caret at an absolute byte offset (collapsed selection).
fn place_cursor(ta: &TextArea, pos: usize) {
    let mut st = ta.edit.borrow_mut();
    st.cursor = pos;
    st.anchor = pos;
}

/// Set an `[anchor, cursor]` selection straight through the shared state.
fn place_selection(ta: &TextArea, anchor: usize, cursor: usize) {
    let mut st = ta.edit.borrow_mut();
    st.anchor = anchor;
    st.cursor = cursor;
}

#[test]
fn ctrl_right_and_left_move_by_word() {
    let mut ta = laid_out("hello world foo", 400.0, 120.0);
    place_cursor(&ta, 0);
    key_mods(&mut ta, Key::ArrowRight, ctrl());
    assert_eq!(ta.cursor(), 5); // end of "hello"
    key_mods(&mut ta, Key::ArrowRight, ctrl());
    assert_eq!(ta.cursor(), 11); // end of "world"
    key_mods(&mut ta, Key::ArrowLeft, ctrl());
    assert_eq!(ta.cursor(), 6); // start of "world"
    assert_eq!(
        ta.selection(),
        None,
        "plain word move collapses the selection"
    );
}

#[test]
fn ctrl_shift_right_extends_selection_by_word() {
    let mut ta = laid_out("hello world", 400.0, 120.0);
    place_cursor(&ta, 0);
    key_mods(&mut ta, Key::ArrowRight, ctrl_shift());
    assert_eq!(ta.cursor(), 5);
    assert_eq!(ta.selection(), Some((0, 5)), "Shift keeps the anchor put");
    key_mods(&mut ta, Key::ArrowRight, ctrl_shift());
    assert_eq!(ta.selection(), Some((0, 11)));
}

#[test]
fn ctrl_backspace_deletes_previous_word() {
    let mut ta = laid_out("hello world", 400.0, 120.0);
    place_cursor(&ta, 11);
    key_mods(&mut ta, Key::Backspace, ctrl());
    assert_eq!(ta.text(), "hello ");
    assert_eq!(ta.cursor(), 6);
}

#[test]
fn ctrl_delete_deletes_next_word() {
    let mut ta = laid_out("hello world", 400.0, 120.0);
    place_cursor(&ta, 0);
    key_mods(&mut ta, Key::Delete, ctrl());
    assert_eq!(ta.text(), " world");
    assert_eq!(ta.cursor(), 0);
}

#[test]
fn word_delete_removes_active_selection_first() {
    // With a selection present, Ctrl+Backspace deletes the selection, not the
    // word before it (standard precedence).
    let mut ta = laid_out("hello world", 400.0, 120.0);
    place_selection(&ta, 0, 5); // "hello"
    key_mods(&mut ta, Key::Backspace, ctrl());
    assert_eq!(ta.text(), " world");
    assert_eq!(ta.cursor(), 0);
    assert_eq!(ta.selection(), None);
}

/// Typing a space at the end of a line must move the caret right by one
/// space-advance. Regression guard for the trailing-whitespace caret bug:
/// the wrap layout trims trailing spaces from `WrappedLine.text`, so the
/// caret x must be measured against the untrimmed source, not the trimmed
/// visual line.
#[test]
fn trailing_space_advances_caret() {
    let mut ta = laid_out("word", 400.0, 120.0);
    place_cursor(&ta, 4);
    let x0 = ta.pos_for_cursor(4).x;

    ta.insert_str(" ");
    ta.layout(Size::new(400.0, 120.0));
    let x1 = ta.pos_for_cursor(5).x;
    assert!(
        x1 > x0 + 1.0,
        "caret must advance past the trailing space: x0={x0} x1={x1}"
    );

    // A second trailing space advances the caret again by the same step.
    ta.insert_str(" ");
    ta.layout(Size::new(400.0, 120.0));
    let x2 = ta.pos_for_cursor(6).x;
    assert!(
        (x2 - x1) > 1.0 && ((x2 - x1) - (x1 - x0)).abs() < 0.5,
        "each trailing space adds one uniform advance: x0={x0} x1={x1} x2={x2}"
    );
}

/// A trailing space typed right at a soft-wrap boundary must not panic and
/// keeps the caret on a valid visual line (matches egui: trailing spaces
/// consumed at the wrap keep the caret adjacent to the last word).
#[test]
fn trailing_space_at_wrap_boundary_no_panic() {
    // Narrow width forces "aaaa bbbb" to wrap into two visual lines.
    let mut ta = laid_out("aaaa bbbb", 60.0, 120.0);
    assert!(ta.visual_line_count() >= 2, "text should wrap");
    // Caret just after the first word, then type a space at the boundary.
    place_cursor(&ta, 4);
    ta.insert_str(" ");
    ta.layout(Size::new(60.0, 120.0));
    // Must not panic and must return a finite position.
    let p = ta.pos_for_cursor(5);
    assert!(p.x.is_finite() && p.y.is_finite());
}

#[test]
fn tab_indents_every_line_of_a_multiline_selection() {
    // Select from col 0 of line 1 into line 2 so two lines are touched. Tab
    // indents both and the same text stays selected.
    let mut ta = laid_out("aa\nbb\ncc", 400.0, 200.0);
    place_selection(&ta, 0, 4);
    key_mods(&mut ta, Key::Tab, Modifiers::default());
    assert_eq!(ta.text(), "    aa\n    bb\ncc");
    // Anchor stayed at the line start (indent falls inside), cursor rode +8.
    assert_eq!(ta.selection(), Some((0, 12)));
}

#[test]
fn shift_tab_outdents_every_line_of_a_multiline_selection() {
    let mut ta = laid_out("    aa\n    bb\ncc", 400.0, 200.0);
    place_selection(&ta, 0, 12);
    key_mods(&mut ta, Key::Tab, shift_mod());
    assert_eq!(ta.text(), "aa\nbb\ncc");
    assert_eq!(ta.selection(), Some((0, 4)), "same text stays selected");
}

#[test]
fn shift_tab_outdents_the_caret_line_without_a_selection() {
    let mut ta = laid_out("    ab", 400.0, 120.0);
    place_cursor(&ta, 6);
    key_mods(&mut ta, Key::Tab, shift_mod());
    assert_eq!(ta.text(), "ab");
    assert_eq!(
        ta.cursor(),
        2,
        "caret column shifts left by the removed indent"
    );
}

#[test]
fn shift_tab_removes_only_the_spaces_present_when_fewer_than_a_tab() {
    // Two leading spaces: outdent removes just those two, not a full tab width.
    let mut ta = laid_out("  ab", 400.0, 120.0);
    place_cursor(&ta, 4);
    key_mods(&mut ta, Key::Tab, shift_mod());
    assert_eq!(ta.text(), "ab");
    assert_eq!(ta.cursor(), 2);
}

#[test]
fn shift_tab_removes_a_single_leading_tab() {
    let mut ta = laid_out("\tab", 400.0, 120.0);
    place_cursor(&ta, 3);
    key_mods(&mut ta, Key::Tab, shift_mod());
    assert_eq!(ta.text(), "ab");
    assert_eq!(ta.cursor(), 2);
}

#[test]
fn tab_without_multiline_selection_inserts_spaces() {
    // A within-line selection is replaced by a single indent (legacy behavior).
    let mut ta = laid_out("abcd", 400.0, 120.0);
    place_selection(&ta, 1, 3); // "bc"
    key_mods(&mut ta, Key::Tab, Modifiers::default());
    assert_eq!(ta.text(), "a    d");
    assert_eq!(ta.cursor(), 5);
}
