//! `TextEditTests.MultiLineTests`: C#'s `InternalTextEditWidget(text, 12,
//! multiLine, 0)` is an agg-gui [`TextArea`] at C#'s 12 points (16 pixels to
//! the em at 96 dpi) with no padding and one em per line (C#'s
//! `TypeFacePrinter.LineSpacing` of 1), sized as C# sizes it: as tall as its
//! lines, its lower-left corner at the container's origin. It is as wide as
//! the 200-wide container so nothing wraps (C# sizes it to its text; every
//! click here lands within the text's first few pixels either way).
//!
//! C#'s single-line `InternalTextEditWidget` of the selection block is the
//! same `TextArea` with one line: the selection API is shared, and agg-gui's
//! `TextField` carries no index setters. `Selecting = true` has no
//! counterpart: a `TextArea` selects whenever its anchor and caret differ.

use std::sync::Arc;

use agg_gui::clipboard;
use agg_gui::event::{Key, Modifiers};
use agg_gui::widgets::TextArea;
use agg_gui::{Rect, Size, Widget};
use agg_gui_automation::HeadlessWindow;

use super::{contains_focus, down, handle, send_char, send_key, shift, up};

/// C#'s 12 points in pixels: `emSizeInPoints / PointsPerInch * PixelsPerInch`.
const EM_PIXELS: f64 = 12.0 / 72.0 * 96.0;

const NAME: &str = "multiLine";

/// C# `new InternalTextEditWidget(text, 12, multiLine, 0)`.
fn internal_text_edit_widget(text: &str) -> TextArea {
    let mut area = TextArea::new(Arc::new(agg_gui::fonts::standard_ui_font()))
        .with_font_size(EM_PIXELS)
        .with_padding(0.0)
        .with_line_spacing(1.0)
        .with_text(text)
        .with_name(NAME);
    area.layout(Size::new(200.0, 10_000.0));
    let height = area.content_height();
    area.set_bounds(Rect::new(0.0, 0.0, 200.0, height));
    area.layout(Size::new(200.0, height));
    area
}

/// C#'s `container` (`LocalBounds = (0, 0, 200, 200)`) holding `multiLine`.
fn container_with(text: &str) -> HeadlessWindow {
    let mut container = HeadlessWindow::new(200.0, 200.0);
    container.add_child(Box::new(internal_text_edit_widget(text)));
    container
}

fn area(c: &HeadlessWindow) -> &TextArea {
    handle(c, NAME)
        .downcast::<TextArea>(c.root())
        .expect("the edit widget is a TextArea")
}

fn height(c: &HeadlessWindow) -> f64 {
    area(c).bounds().height
}

fn char_index(c: &HeadlessWindow) -> usize {
    area(c).char_index_to_insert_before()
}

fn insert_bar(c: &HeadlessWindow) -> agg_gui::Point {
    area(c).insert_bar_position()
}

fn click(c: &mut HeadlessWindow, x: f64, y: f64) {
    down(c, 1, x, y);
    up(c, x, y);
}

fn key(c: &mut HeadlessWindow, key: Key) {
    send_key(c, key, Modifiers::default());
}

#[test]
fn multi_line_tests() {
    let _clipboard = clipboard::simulate();

    // make sure selection ranges are always working
    {
        let _clipboard = clipboard::simulate();

        let mut single_line = internal_text_edit_widget("test");

        let mut test_range = |start: i32, end: i32, expected: &str| {
            single_line.set_char_index_to_insert_before(start);
            single_line.set_selection_index_to_start_before(end);
            assert_eq!(single_line.selected_text(), expected);
            single_line.copy_selection();

            assert_eq!(clipboard::get_text().unwrap_or_default(), expected);
        };

        // ask for some selections
        test_range(-10, -8, "");
        test_range(-8, -10, "");
        test_range(18, 10, "");
        test_range(10, 18, "");
        test_range(2, -10, "te");
        test_range(-10, 2, "te");
        test_range(18, 2, "st");
        test_range(3, 22, "t");
    }

    {
        let single_line = internal_text_edit_widget("test");
        let multi_line = internal_text_edit_widget("test\ntest\ntest");
        assert!(multi_line.bounds().height >= single_line.bounds().height * 3.0);
    }

    // we get the typed results we expect
    {
        let mut container = container_with("\n\n\n\n");
        let c = &mut container;

        click(c, 1.0, 1.0);
        assert!(contains_focus(c, NAME));
        assert!(area(c).selection_index_to_start_before() == 4);
        assert!(text_of(c) == "\n\n\n\n");
        send_char(c, 'a');
        assert!(text_of(c) == "\n\n\n\na");
        key(c, Key::ArrowUp);
        send_char(c, 'a');
        assert!(text_of(c) == "\n\n\na\na");

        let h = height(c);
        click(c, 1.0, h - 1.0);
        assert!(contains_focus(c, NAME));
        assert!(area(c).selection_index_to_start_before() == 0);
        assert!(text_of(c) == "\n\n\na\na");
        send_char(c, 'a');
        assert!(text_of(c) == "a\n\n\na\na");
        key(c, Key::ArrowDown);
        c.on_key_down(Key::Char('A'), shift());
        assert!(text_of(c) == "a\nA\n\na\na");
    }

    // make sure the insert position is correct when homed
    {
        let mut container = container_with("line1\nline2\nline3");
        let c = &mut container;

        click(c, 5.0, 1.0);
        assert!(contains_focus(c, NAME));
        assert!(insert_bar(c).y == -32.0);
        key(c, Key::Home);
        assert!(insert_bar(c).y == -32.0);
        assert!(text_of(c) == "line1\nline2\nline3");
        send_char(c, 'a');
        assert!(text_of(c) == "line1\nline2\naline3");
        key(c, Key::Backspace);
        assert!(text_of(c) == "line1\nline2\nline3");
        assert!(insert_bar(c).y == -32.0);
    }

    // make sure the insert position is correct when move left to end of line
    {
        let mut container = container_with("xx");
        let c = &mut container;

        click(c, 1.0, 1.0);
        assert!(contains_focus(c, NAME));
        assert!(char_index(c) == 0);
        assert!(insert_bar(c).x == 0.0);
        key(c, Key::Home);
        assert!(char_index(c) == 0);
        assert!(insert_bar(c).x == 0.0);
        key(c, Key::ArrowRight);
        assert!(char_index(c) == 1);
        let left_one = insert_bar(c).x;
        key(c, Key::ArrowRight);
        assert!(char_index(c) == 2);
        assert!(insert_bar(c).x == left_one * 2.0);
    }

    // make sure the cursor is at the right hight when it is after a \n that is on the first line
    {
        let multi_line = internal_text_edit_widget("\n1\n\n3\n");
        assert!(multi_line.bounds().height == 16.0 * 5.0);
        let mut container = HeadlessWindow::new(200.0, 200.0);
        container.add_child(Box::new(multi_line));
        let c = &mut container;

        let h = height(c);
        click(c, 1.0, h - 1.0);

        assert!(char_index(c) == 0);
        assert!(insert_bar(c).y == 0.0);

        // move past \n
        key(c, Key::ArrowRight);
        assert!(char_index(c) == 1);
        assert!(insert_bar(c).y == -16.0);

        // move past 1
        key(c, Key::ArrowRight);
        assert!(char_index(c) == 2);
        assert!(insert_bar(c).y == -16.0);

        // move past \n
        key(c, Key::ArrowRight);
        assert!(char_index(c) == 3);
        assert!(insert_bar(c).y == -32.0);

        // move past \n
        key(c, Key::ArrowRight);
        assert!(char_index(c) == 4);
        assert!(insert_bar(c).y == -48.0);

        // move past 3
        key(c, Key::ArrowRight);
        assert!(char_index(c) == 5);
        assert!(insert_bar(c).y == -48.0);

        // move past \n
        key(c, Key::ArrowRight);
        assert!(char_index(c) == 6);
        assert!(insert_bar(c).y == -64.0);
    }
}

/// C# `multiLine.Text`.
fn text_of(c: &HeadlessWindow) -> String {
    area(c).text()
}
