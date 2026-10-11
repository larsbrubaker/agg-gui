//! Tests for `MarkdownView`'s matching-text highlight (`matching.rs`).
//!
//! `matching_text_pipeline_still_renders_tables` ports agg-sharp's
//! `MarkdownTableTests.MatchingTextPipelineStillRendersTables`; the rest pin
//! where matches are boxed (paragraphs, headings, list items, links, table
//! cells, mid-word and multi-word) and where they are not (inline code, code
//! blocks, an unset or whitespace-only matching text).

use std::sync::Arc;

use crate::geometry::Size;
use crate::tests::paint_recorder::PaintRecorder;
use crate::text::Font;
use crate::widget::Widget;

use super::super::{LayoutItem, LineRun, LineStyle, MarkdownView};
use super::{match_fill, match_ranges, MATCH_PAD};

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn view(markdown: &str, matching: Option<&str>) -> MarkdownView {
    crate::widget::set_current_viewport(Size::new(480.0, 320.0));
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
    let mut view = MarkdownView::new(markdown, font);
    view.set_matching_text(matching);
    view.layout(Size::new(480.0, 2000.0));
    view
}

/// `(text, highlighted, style, link)` of every text run, in order.
fn runs(view: &MarkdownView) -> Vec<(String, bool, LineStyle, Option<String>)> {
    let mut out = Vec::new();
    for item in &view.items {
        if let LayoutItem::Line { runs, style, .. } = item {
            for run in runs {
                if let LineRun::Text {
                    text,
                    highlight,
                    link,
                    ..
                } = run
                {
                    out.push((text.clone(), *highlight, *style, link.clone()));
                }
            }
        }
    }
    out
}

fn highlighted(view: &MarkdownView) -> Vec<String> {
    runs(view)
        .into_iter()
        .filter(|(_, h, _, _)| *h)
        .map(|(t, _, _, _)| t)
        .collect()
}

fn match_fills(view: &mut MarkdownView) -> usize {
    let mut ctx = PaintRecorder::new();
    let accent = crate::draw_ctx::DrawCtx::visuals(&ctx).accent;
    view.paint(&mut ctx);
    ctx.fills_with(match_fill(accent))
}

#[test]
fn matching_text_pipeline_still_renders_tables() {
    let view = view(
        "| Name | Qty |\n| --- | --- |\n| Bolt | 4 |\n| Nut | 8 |",
        Some("Bolt"),
    );
    let tables = view
        .items
        .iter()
        .filter(|item| matches!(item, LayoutItem::Table { .. }))
        .count();
    assert_eq!(tables, 1);
}

#[test]
fn match_ranges_are_case_insensitive_substrings_left_to_right() {
    assert_eq!(match_ranges("Bolt the bolts", "bolt"), vec![0..4, 9..13]);
    assert_eq!(match_ranges("aaaa", "aa"), vec![0..2, 2..4]);
    assert_eq!(match_ranges("Größe GRÖSSE", "größe"), vec![0..7]);
    assert!(match_ranges("nut", "bolt").is_empty());
    assert!(match_ranges("bolt", "").is_empty());
}

#[test]
fn paragraph_match_keeps_the_documents_own_text() {
    let view = view("Bolt the bracket down.", Some("bolt"));
    assert_eq!(highlighted(&view), vec!["Bolt".to_string()]);
    let all: String = runs(&view).into_iter().map(|(t, ..)| t).collect();
    assert_eq!(all, "Bolt the bracket down.");
}

#[test]
fn mid_word_match_is_its_own_padded_box() {
    let view = view("unbolted", Some("BOLT"));
    let LayoutItem::Line { runs, .. } = &view.items[0] else {
        panic!("first item is a line");
    };
    let mut x_end = 0.0;
    let mut parts = Vec::new();
    for run in runs {
        let LineRun::Text {
            text,
            highlight,
            lead,
            trail,
            x,
            width,
            ..
        } = run
        else {
            panic!("text run");
        };
        assert!((x - x_end).abs() < 1e-9, "runs sit side by side");
        x_end = x + width;
        parts.push((text.clone(), *highlight, *lead, *trail));
    }
    assert_eq!(
        parts,
        vec![
            ("un".to_string(), false, 0.0, 0.0),
            ("bolt".to_string(), true, MATCH_PAD, MATCH_PAD),
            ("ed".to_string(), false, 0.0, 0.0),
        ]
    );
    let plain = view_width("unbolted", None);
    assert!((x_end - plain - 2.0 * MATCH_PAD).abs() < 0.5);
}

fn view_width(markdown: &str, matching: Option<&str>) -> f64 {
    let view = view(markdown, matching);
    let LayoutItem::Line { runs, .. } = &view.items[0] else {
        panic!("first item is a line");
    };
    runs.iter()
        .map(|run| match run {
            LineRun::Text { width, .. } => *width,
            _ => 0.0,
        })
        .sum()
}

#[test]
fn multi_word_match_is_one_box() {
    let view = view("Level the print bed first.", Some("print bed"));
    assert_eq!(highlighted(&view), vec!["print bed".to_string()]);
}

#[test]
fn heading_match_is_highlighted() {
    let view = view("## Bolt sizes", Some("bolt"));
    let runs = runs(&view);
    assert!(runs
        .iter()
        .any(|(t, h, style, _)| t == "Bolt" && *h && *style == LineStyle::H2));
}

#[test]
fn list_item_match_is_highlighted() {
    let view = view("- Washer\n- Bolt", Some("bolt"));
    assert_eq!(highlighted(&view), vec!["Bolt".to_string()]);
}

#[test]
fn link_text_match_is_highlighted_and_stays_a_link() {
    let view = view("See [bolt sizes](https://example.com).", Some("bolt"));
    let runs = runs(&view);
    assert!(runs.iter().any(|(t, h, _, link)| t == "bolt"
        && *h
        && link.as_deref() == Some("https://example.com")));
}

#[test]
fn inline_code_and_code_blocks_are_not_highlighted() {
    let mut view = view("Use `bolt()` here.\n\n```\nbolt\n```", Some("bolt"));
    assert!(highlighted(&view).is_empty());
    assert_eq!(match_fills(&mut view), 0);
}

#[test]
fn table_cell_match_is_highlighted() {
    let mut view = view(
        "| Name | Qty |\n| --- | --- |\n| Bolt | 4 |\n| Nut | 8 |",
        Some("bolt"),
    );
    assert_eq!(match_fills(&mut view), 1);
}

#[test]
fn paragraph_match_paints_one_box() {
    let mut view = view("Tighten the bolt.", Some("bolt"));
    assert_eq!(match_fills(&mut view), 1);
}

#[test]
fn unset_or_blank_matching_text_changes_nothing() {
    let plain = runs(&view("Tighten the bolt.", None));
    assert_eq!(runs(&view("Tighten the bolt.", Some("   "))), plain);
    let mut set_then_cleared = view("Tighten the bolt.", Some("bolt"));
    assert_eq!(set_then_cleared.matching_text(), Some("bolt"));
    set_then_cleared.set_matching_text(None);
    set_then_cleared.layout(Size::new(480.0, 2000.0));
    assert_eq!(runs(&set_then_cleared), plain);
    assert_eq!(match_fills(&mut set_then_cleared), 0);
    assert_eq!(
        view_width("Tighten the bolt.", None),
        view_width("Tighten the bolt.", Some(" "))
    );
}

#[test]
fn highlight_does_not_change_the_selectable_text() {
    let plain = view("Tighten the unbolted print bed.", None);
    let lit = view("Tighten the unbolted print bed.", Some("bolt"));
    assert_eq!(plain.selectable_text, lit.selectable_text);
}
