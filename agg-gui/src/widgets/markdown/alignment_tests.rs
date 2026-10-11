//! Tests ported 1:1 from agg-sharp
//! `Tests/Agg.Tests/MarkdigAgg/MarkdownAlignmentTests.cs`: GitHub-style
//! alignment HTML (the form the markdown editor stores) renders aligned in
//! the viewer (`alignment.rs`).
//!
//! C# finds the `ParagraphX` / `HeadingRowX` holding a word among the child
//! widgets and reads its `ContentHAnchor`; here the same comes from
//! [`MarkdownView::text_blocks`], a block's level telling a heading from a
//! paragraph.

use std::sync::Arc;

use crate::geometry::Size;
use crate::text::Font;
use crate::widget::Widget;

use super::{MarkdownAlign, MarkdownTextBlock, MarkdownView};

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/LiberationSans-Regular.ttf");

fn render_markdown(markdown: &str) -> MarkdownView {
    crate::widget::set_current_viewport(Size::new(2000.0, 600.0));
    let mut view = MarkdownView::new(
        markdown,
        Arc::new(Font::from_slice(TEST_FONT).expect("test font")),
    );
    view.layout(Size::new(2000.0, 2000.0));
    view
}

fn has_word(block: &MarkdownTextBlock, word: &str) -> bool {
    block.words.iter().any(|w| w.text == word)
}

/// C# `RowContaining<ParagraphX>` (`heading == false`) or `<HeadingRowX>`:
/// the single block of that kind holding `word`.
fn row_containing(view: &MarkdownView, word: &str, heading: bool) -> MarkdownTextBlock {
    let rows: Vec<_> = view
        .text_blocks()
        .into_iter()
        .filter(|block| (block.level > 0) == heading && has_word(block, word))
        .collect();
    assert_eq!(rows.len(), 1, "one row holds {word}: {rows:?}");
    rows.into_iter().next().unwrap_or_else(|| unreachable!())
}

fn any_word(view: &MarkdownView, matches: impl Fn(&str) -> bool) -> bool {
    view.text_blocks()
        .iter()
        .any(|block| block.words.iter().any(|w| matches(&w.text)))
}

fn word_x(view: &MarkdownView, word: &str) -> f64 {
    let xs: Vec<f64> = view
        .text_blocks()
        .iter()
        .flat_map(|block| block.words.iter())
        .filter(|w| w.text == word)
        .map(|w| w.x)
        .collect();
    assert_eq!(xs.len(), 1, "one {word}");
    xs[0]
}

#[test]
fn div_align_pair_aligns_the_blocks_between() {
    let view = render_markdown(
        "Before\n\n<div align=\"center\">\n\n# Title\n\nCentered\n\n</div>\n\n\
         <div align=\"right\">\n\nRighted\n\n</div>\n\nAfter",
    );

    assert_eq!(
        row_containing(&view, "Title", true).align,
        MarkdownAlign::Center
    );
    assert_eq!(
        row_containing(&view, "Centered", false).align,
        MarkdownAlign::Center
    );
    assert_eq!(
        row_containing(&view, "Righted", false).align,
        MarkdownAlign::Right
    );
    assert_eq!(
        row_containing(&view, "Before", false).align,
        MarkdownAlign::Left
    );
    assert_eq!(
        row_containing(&view, "After", false).align,
        MarkdownAlign::Left
    );
}

#[test]
fn centered_word_sits_right_of_a_left_aligned_one() {
    let mut view = render_markdown(
        "Left\n\n<div align=\"center\">\n\nMiddle\n\n</div>\n\n\
         <div align=\"right\">\n\nEdge\n\n</div>",
    );

    view.layout(Size::new(600.0, 2000.0));

    let left = word_x(&view, "Left");
    let middle = word_x(&view, "Middle");
    let edge = word_x(&view, "Edge");

    assert!(middle > left + 100.0, "{middle} > {left} + 100");
    assert!(edge > middle + 100.0, "{edge} > {middle} + 100");
}

#[test]
fn unmatched_tags_degrade_gracefully() {
    let view = render_markdown(
        "</div>\n\nPlain\n\n<div>\n\n<div align=\"center\">\n\nInner\n\n</div>\n\n\
         StillPlain\n\n<div align=\"right\">\n\nToTheEnd",
    );

    assert_eq!(
        row_containing(&view, "Plain", false).align,
        MarkdownAlign::Left
    );
    assert_eq!(
        row_containing(&view, "Inner", false).align,
        MarkdownAlign::Center
    );
    // The inner </div> closes the centered div, not the outer plain one.
    assert_eq!(
        row_containing(&view, "StillPlain", false).align,
        MarkdownAlign::Left
    );
    assert_eq!(
        row_containing(&view, "ToTheEnd", false).align,
        MarkdownAlign::Right
    );
}

#[test]
fn one_line_aligned_elements_render_their_text() {
    let view = render_markdown(
        "<p align=\"center\">Hello &amp; <b>world</b></p>\n\n\
         <h2 align=\"right\">Title</h2>\n\n<p>NotShown</p>",
    );

    let paragraph = row_containing(&view, "Hello", false);
    assert_eq!(paragraph.align, MarkdownAlign::Center);
    assert!(has_word(&paragraph, "&"));
    assert!(has_word(&paragraph, "world"));

    let heading = row_containing(&view, "Title", true);
    assert_eq!(heading.align, MarkdownAlign::Right);
    assert_eq!(heading.level, 2);

    // HTML other than alignment keeps showing nothing.
    assert!(!any_word(&view, |w| w.contains("NotShown")));
}

#[test]
fn one_line_aligned_div_shows_its_text() {
    // AI-written notes often center a line as a one-line div; it used to render as nothing.
    let view = render_markdown("<div align=\"center\">Centered: version 3</div>\n");

    let paragraph = row_containing(&view, "Centered:", false);
    assert_eq!(paragraph.align, MarkdownAlign::Center);
}

#[test]
fn markup_only_element_adds_no_empty_row() {
    let view = render_markdown("<p align=\"center\"><img src=\"logo.png\"></p>");

    assert!(view.text_blocks().iter().all(|block| block.level > 0));
}

#[test]
fn data_align_attribute_does_not_align() {
    let view = render_markdown(
        "<div data-align=\"center\">\n\nPlain\n\n</div>\n\n<p data-align=\"center\">NotShown</p>",
    );

    assert_eq!(
        row_containing(&view, "Plain", false).align,
        MarkdownAlign::Left
    );
    assert!(!any_word(&view, |w| w == "NotShown"));
}

#[test]
fn two_elements_on_one_line_are_not_glued_together() {
    let view = render_markdown("<p align=\"center\">a</p><p>b</p>");

    assert!(!any_word(&view, |w| w.contains('b')));
}
