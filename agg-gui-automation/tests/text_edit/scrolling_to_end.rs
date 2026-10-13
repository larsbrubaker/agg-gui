//! `TextEditTests.ScrollingToEndShowsEnd`: a field too narrow for its text
//! shows the start until End scrolls it to show the end.
//!
//! As in C#, "This" and "string" are drawn on their own (a white `Label` in
//! a window the size of the word, standing for C#'s `TextWidget` back
//! buffer) and least-squares matched against the container's pixels
//! (`find_least_squares_match`, C#'s `FindLeastSquaresMatch`): a match under
//! 2,000,000 is "this word is drawn in the field", one over is "it is not".
//! The container is a 110 x 30 window under a white backdrop (C#'s
//! `BackgroundColor = Color.White`). C# draws the container's own back
//! buffer; here it is the headless driver's painted frame.
//!
//! `rust_only_scrolling_to_end_moves_the_words_edges_with_the_field` pins the
//! same scroll through the field's text geometry
//! (`TextField::line_bounds_at`, which folds in the sideways scroll).

use std::sync::Arc;

use agg_gui::event::{Key, Modifiers};
use agg_gui::widgets::{Label, TextFieldTheme};
use agg_gui::{Color, Framebuffer, Rect, Size, Widget};
use agg_gui_automation::{find_least_squares_match, HeadlessWindow, ProbeWidget, UiDriver};

use super::{
    container_with, contains_focus, down, field, send_key, text_edit_widget, up, POINT_SIZE,
};

const NAME: &str = "editField1";
const TEXT: &str = "This is a nice long text string";
/// C#'s limit between "drawn" and "not drawn", on `bestLeastSquares`.
const MATCH_LIMIT: f64 = 2_000_000.0;

/// A copy of a painted frame (`Framebuffer` is not `Clone`).
fn copy_of(screen: &Framebuffer) -> Framebuffer {
    let mut copy = Framebuffer::new(screen.width(), screen.height());
    copy.pixels_mut().copy_from_slice(screen.pixels());
    copy
}

/// C# `new TextWidget(word)` drawn on a white back buffer of its own size.
fn word_image(word: &str) -> Framebuffer {
    let mut label = Label::new(word, Arc::new(agg_gui::fonts::standard_ui_font()))
        .with_font_size(POINT_SIZE)
        .with_color(Color::black());
    let size = label.layout(Size::new(1000.0, 1000.0));
    label.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    let mut window = HeadlessWindow::new(size.width, size.height);
    window.add_child(Box::new(
        ProbeWidget::new("white")
            .with_bounds(Rect::new(0.0, 0.0, size.width, size.height))
            .with_background(Color::white()),
    ));
    window.add_child(Box::new(label));
    window.draw();
    copy_of(window.driver().current_screen())
}

/// C# `container.OnDraw(container.BackBuffer.NewGraphics2D())`, then the
/// container's pixels.
fn container_image(container: &mut HeadlessWindow) -> Framebuffer {
    container.draw();
    copy_of(container.driver().current_screen())
}

/// C# `container.BackBuffer.FindLeastSquaresMatch(word, out _, out best)`.
fn best_least_squares(container: &Framebuffer, word: &Framebuffer) -> f64 {
    find_least_squares_match(container, word, f64::MAX).best_least_squares
}

/// The 110 x 30 container of the test with its edit field, under a white
/// backdrop.
fn white_container() -> HeadlessWindow {
    let mut container = HeadlessWindow::new(110.0, 30.0);
    container.add_child(Box::new(
        ProbeWidget::new("white")
            .with_bounds(Rect::new(0.0, 0.0, 110.0, 30.0))
            .with_background(Color::white()),
    ));
    // C#'s edit widget draws only its text, black, over its container's white
    let edit = text_edit_widget(NAME, TEXT, 0.0, 0.0, 100.0)
        .with_frame(false)
        .with_theme(TextFieldTheme {
            text_color: Some(Color::black()),
            ..TextFieldTheme::default()
        });
    container.add_child(Box::new(edit));
    container
}

#[test]
fn scrolling_to_end_shows_end() {
    let first_word_text = word_image("This");
    let last_word_text = word_image("string");
    // C# `new TextEditWidget("This is a nice long text string", 0, 0, pixelWidth: 100)`
    let mut container = white_container();

    down(&mut container, 1, 1.0, 1.0);
    up(&mut container, 1.0, 1.0);
    assert!(contains_focus(&container, NAME));

    let screen = container_image(&mut container);
    assert!(best_least_squares(&screen, &first_word_text) < MATCH_LIMIT);
    assert!(best_least_squares(&screen, &last_word_text) > MATCH_LIMIT);

    send_key(&mut container, Key::End, Modifiers::default());

    let screen = container_image(&mut container);
    assert!(best_least_squares(&screen, &first_word_text) > MATCH_LIMIT);
    assert!(best_least_squares(&screen, &last_word_text) < MATCH_LIMIT);
}

/// Where the word at byte range `start..end` is drawn, as field-local x of
/// its left and right edges.
fn word_span(container: &HeadlessWindow, start: usize, end: usize) -> (f64, f64) {
    let f = field(container, NAME);
    (f.line_bounds_at(start).x, f.line_bounds_at(end).x)
}

#[test]
fn rust_only_scrolling_to_end_moves_the_words_edges_with_the_field() {
    let mut container = container_with(
        110.0,
        30.0,
        vec![text_edit_widget(NAME, TEXT, 0.0, 0.0, 100.0)],
    );
    let width = field(&container, NAME).bounds().width;
    let first_word = (0, "This".len());
    let last_word = (TEXT.len() - "string".len(), TEXT.len());

    down(&mut container, 1, 1.0, 1.0);
    up(&mut container, 1.0, 1.0);

    let (left, right) = word_span(&container, first_word.0, first_word.1);
    assert!(left >= 0.0 && right <= width);
    let (left, _) = word_span(&container, last_word.0, last_word.1);
    assert!(left >= width);

    send_key(&mut container, Key::End, Modifiers::default());

    let (_, right) = word_span(&container, first_word.0, first_word.1);
    assert!(right <= 0.0);
    let (left, right) = word_span(&container, last_word.0, last_word.1);
    assert!(left >= 0.0 && right <= width);
}
