//! Tests for `MarkdownView`'s selection model and copy (`selection.rs`,
//! `rich_html.rs`): HTML escaping, hit-testing at fragment edges and on
//! images, and the plain text and styled HTML a copy writes.

use std::sync::Arc;

use crate::geometry::{Point, Size};
use crate::text::Font;

use super::selection::{escape_html, SelectableKind};
use super::MarkdownView;

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

#[test]
fn html_escapes_text_and_gaps() {
    assert_eq!(escape_html("<a&b>"), "&lt;a&amp;b&gt;");
}

/// A copy writes what the user sees, as MarkdigAgg's
/// `GetSelectionClipboardData` does: the selected words as plain text
/// (no `# `, `[..](..)` or backticks), and HTML styled by
/// `StyleHtmlFragment`.  (The view used to copy its selection as
/// Markdown source; that differed from C#, which this test now pins.)
#[test]
fn copy_payloads_are_the_visible_words_and_styled_html() {
    crate::widget::set_current_viewport(Size::new(480.0, 320.0));
    let mut view = MarkdownView::new(
        "# Markdown\n\nParagraph with [link](https://example.com) and `code`.\n\n```text\nlet x = 1;\n```\n\n| A | B |\n| --- | --- |\n| C | D |\n\n![Alt](image.png)",
        test_font(),
    );
    view.layout_markdown(Size::new(480.0, 2000.0));

    // A selected heading word: the word, and an <h1> styled as C# does.
    let (plain, html) = view.copy_payloads(0.."Markdown".len());
    assert_eq!(plain, "Markdown");
    assert_eq!(
        html,
        "<div style=\"font-family:Arial, Helvetica, sans-serif;font-size:11pt;line-height:1.45;color:#333333;background-color:#FFFFFF;\">\
         <h1 style=\"margin:0 0 12px 0;font-size:20pt;font-weight:700;line-height:1.25;\">Markdown</h1></div>"
    );

    // Paragraph through the table: the words, no markdown syntax.
    let text = view.laid_out_text().to_string();
    let start = text.find("Paragraph").expect("paragraph");
    let end = text.find('D').expect("cell D") + 1;
    let (plain, html) = view.copy_payloads(start..end);
    assert_eq!(
        plain,
        "Paragraph with link and code.\nlet x = 1;\nA\tB\nC\tD"
    );
    assert!(
        html.contains("<a href=\"https://example.com\" style="),
        "{html}"
    );
    assert!(html.contains(">link</a>"), "{html}");
    assert!(html.contains(">code</code>"), "{html}");
    assert!(html.contains("<pre style="), "{html}");
    assert!(html.contains("let x = 1;"), "{html}");
    assert!(html.contains("<table style="), "{html}");
    assert!(html.contains("<th style="), "{html}");
    assert!(html.contains(">C</td>"), "{html}");
    assert!(!html.contains("<!--StartFragment-->"));

    // Everything: the whole document's styled HTML (C# `ToStyledHtml`).
    view.select_all_text();
    let (plain, html) = view.copy_payloads(view.selection_range().expect("selection"));
    assert_eq!(plain, view.laid_out_text());
    assert_eq!(
        html,
        super::to_styled_html(&view.markdown, &Default::default())
    );
}

#[test]
fn hit_testing_clamps_to_fragment_edges() {
    crate::widget::set_current_viewport(Size::new(320.0, 200.0));
    let mut view = MarkdownView::new("café", test_font());
    view.layout_markdown(Size::new(320.0, 1000.0));
    let fragment = view
        .selectable_fragments
        .iter()
        .find(|fragment| fragment.text.contains("café"))
        .expect("text fragment")
        .clone();

    let y = fragment.y + fragment.height * 0.5;
    assert_eq!(
        view.text_pos_at(Point::new(fragment.text_x - 20.0, y)),
        Some(fragment.range.start)
    );
    assert_eq!(
        view.text_pos_at(Point::new(fragment.text_x + fragment.width + 20.0, y)),
        Some(fragment.range.end)
    );
}

#[test]
fn image_hit_test_targets_inline_images() {
    crate::widget::set_current_viewport(Size::new(320.0, 200.0));
    let mut view = MarkdownView::new("![Alt](image.png)", test_font());
    view.layout_markdown(Size::new(320.0, 1000.0));
    let fragment = view
        .selectable_fragments
        .iter()
        .find(|fragment| matches!(fragment.kind, SelectableKind::Image { .. }))
        .expect("image fragment")
        .clone();
    let hit = view.hit_image(Point::new(
        fragment.text_x + fragment.width * 0.5,
        fragment.y + fragment.height * 0.5,
    ));

    assert_eq!(hit, Some(("image.png".to_string(), "Alt".to_string(), 0)));
}

#[test]
fn copy_payloads_render_ordered_and_bullet_lists_as_html_lists() {
    crate::widget::set_current_viewport(Size::new(480.0, 320.0));
    let mut view = MarkdownView::new(
        "1. First item\n2. Second item with [link](https://example.com)\n\n- Alpha\n- Beta",
        test_font(),
    );
    view.layout_markdown(Size::new(480.0, 2000.0));
    view.select_all_text();

    let (_, html) = view.copy_payloads(view.selection_range().expect("selection"));

    assert!(html.contains("<ol"));
    assert!(html.contains("<li"));
    assert!(html.contains(">First item"));
    assert!(html.contains(">Second item"));
    assert!(html.contains("<a href=\"https://example.com\""));
    assert!(html.contains("<ul"));
    assert!(html.contains(">Alpha"));
    assert!(html.contains(">Beta"));
    assert!(!html.contains(">1. First item"));
    assert!(!html.contains(">• Alpha"));
}
