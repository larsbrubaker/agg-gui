//! `TextEditTests.ScrollingToEndShowsEnd`: a field too narrow for its text
//! shows the start until End scrolls it to show the end.
//!
//! C# renders "This" and "string" on their own and least-squares matches
//! each against the container's pixels: a match under 2,000,000 is "this word
//! is drawn in the field", one over is "it is not". Here a word is drawn in
//! the field when both its edges, read through the field's own text
//! geometry (`TextField::line_bounds_at`, which folds in the sideways
//! scroll), lie inside the field's width; not drawn when neither part of it
//! does. Only the field's width matters, so the 110 x 30 container's
//! pixels have no counterpart.

use agg_gui::event::{Key, Modifiers};
use agg_gui::Widget;

use super::{container_with, contains_focus, down, field, send_key, text_edit_widget, up};

const NAME: &str = "editField1";
const TEXT: &str = "This is a nice long text string";

/// Where the word at byte range `start..end` is drawn, as field-local x of
/// its left and right edges.
fn word_span(
    container: &agg_gui_automation::HeadlessWindow,
    start: usize,
    end: usize,
) -> (f64, f64) {
    let f = field(container, NAME);
    (f.line_bounds_at(start).x, f.line_bounds_at(end).x)
}

fn shown(container: &agg_gui_automation::HeadlessWindow, start: usize, end: usize) -> bool {
    let (left, right) = word_span(container, start, end);
    let width = field(container, NAME).bounds().width;
    left >= 0.0 && right <= width
}

fn hidden(container: &agg_gui_automation::HeadlessWindow, start: usize, end: usize) -> bool {
    let (left, right) = word_span(container, start, end);
    let width = field(container, NAME).bounds().width;
    right <= 0.0 || left >= width
}

#[test]
fn scrolling_to_end_shows_end() {
    // C# `new TextEditWidget("This is a nice long text string", 0, 0, pixelWidth: 100)`
    let mut container = container_with(
        110.0,
        30.0,
        vec![text_edit_widget(NAME, TEXT, 0.0, 0.0, 100.0)],
    );
    let first_word = (0, "This".len());
    let last_word = (TEXT.len() - "string".len(), TEXT.len());

    down(&mut container, 1, 1.0, 1.0);
    up(&mut container, 1.0, 1.0);
    assert!(contains_focus(&container, NAME));

    assert!(shown(&container, first_word.0, first_word.1));
    assert!(hidden(&container, last_word.0, last_word.1));

    send_key(&mut container, Key::End, Modifiers::default());

    assert!(hidden(&container, first_word.0, first_word.1));
    assert!(shown(&container, last_word.0, last_word.1));
}
