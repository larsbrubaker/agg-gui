//! Tests for [`MarkdownView::max_line_width`] (agg-sharp's
//! `ParagraphX.MaxLineWidth`) and [`MarkdownView::laid_out_text`]: the widest
//! row is measured from its text edge, a view re-laid at that width keeps its
//! rows, list indents count, and the plain text carries no markdown syntax.

use std::sync::Arc;

use crate::geometry::Size;
use crate::text::{measure_text_metrics, Font};
use crate::widget::Widget;

use super::{LayoutItem, MarkdownView};

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

fn laid_out(markdown: &str, width: f64) -> MarkdownView {
    crate::widget::set_current_viewport(Size::new(800.0, 600.0));
    let mut view = MarkdownView::new(markdown, font());
    view.layout(Size::new(width, 2000.0));
    view
}

fn row_count(view: &MarkdownView) -> usize {
    view.items
        .iter()
        .filter(|item| matches!(item, LayoutItem::Line { runs, .. } if !runs.is_empty()))
        .count()
}

#[test]
fn widest_row_is_the_longest_line_of_text() {
    let view = laid_out("Fillet\n\nAt least one part", 350.0);
    let widest = measure_text_metrics(&font(), "At least one part", 14.0).width;
    assert!(
        (view.max_line_width() - widest).abs() < 1e-9,
        "max_line_width {} vs measured {}",
        view.max_line_width(),
        widest
    );
}

#[test]
fn relaying_at_the_widest_row_keeps_every_row() {
    let text =
        "A tooltip paragraph long enough that a narrow view wraps it onto several rows of text.";
    let view = laid_out(text, 200.0);
    let rows = row_count(&view);
    assert!(rows > 1, "the paragraph wraps at 200");
    assert!(view.max_line_width() <= 200.0 - 16.0);

    let fitted = view.max_line_width() + 16.0;
    let refit = laid_out(text, fitted);
    assert_eq!(row_count(&refit), rows);
    assert_eq!(refit.max_line_width(), view.max_line_width());
}

#[test]
fn list_indent_counts_toward_the_widest_row() {
    let plain = laid_out("Item", 350.0);
    let listed = laid_out("- Item", 350.0);
    assert!(listed.max_line_width() > plain.max_line_width());
}

#[test]
fn empty_document_has_no_width() {
    assert_eq!(laid_out("", 350.0).max_line_width(), 0.0);
}

#[test]
fn laid_out_text_drops_markdown_syntax() {
    let view = laid_out("Fillet\n\n*At least 1 part must be selected*", 350.0);
    let text = view.laid_out_text();
    assert!(
        text.contains("At least 1 part must be selected"),
        "{text:?}"
    );
    assert!(!text.contains('*'), "{text:?}");
}
