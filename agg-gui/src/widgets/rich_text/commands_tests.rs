//! Unit tests for the rich-text command engine ([`super`]).

use super::*;
use crate::color::Color;
use crate::widgets::rich_text::model::{Block, DocPos, DocRange, ListKind, RichDoc};
use crate::widgets::text_area::TextHAlign;

fn doc_one(text: &str) -> RichDoc {
    RichDoc::from_blocks(vec![Block::plain(text)])
}

fn full_range(doc: &RichDoc, block: usize) -> DocRange {
    DocRange::new(
        DocPos::new(block, 0),
        DocPos::new(block, doc.blocks[block].text_len()),
    )
}

#[test]
fn toggle_bold_sets_then_clears() {
    let mut doc = doc_one("hello");
    let r = full_range(&doc, 0);
    apply_command(&mut doc, r, &RichCommand::ToggleBold);
    assert!(doc.blocks[0].runs.iter().all(|run| run.style.bold));
    // Whole range already bold -> toggles off.
    apply_command(&mut doc, r, &RichCommand::ToggleBold);
    assert!(doc.blocks[0].runs.iter().all(|run| !run.style.bold));
}

#[test]
fn toggle_over_mixed_range_sets_all() {
    // "abcde" with "cd" already bold -> whole-range toggle should SET bold
    // everywhere (since not the entire range is bold).
    let mut doc = doc_one("abcde");
    apply_command(
        &mut doc,
        DocRange::new(DocPos::new(0, 2), DocPos::new(0, 4)),
        &RichCommand::ToggleBold,
    );
    // Now toggle across the whole range.
    let r = full_range(&doc, 0);
    apply_command(&mut doc, r, &RichCommand::ToggleBold);
    assert!(doc.blocks[0].runs.iter().all(|run| run.style.bold));
    // And normalization collapsed it back to one run.
    assert_eq!(doc.blocks[0].runs.len(), 1);
}

#[test]
fn apply_to_subrange_splits_at_edges() {
    let mut doc = doc_one("abcdef");
    apply_command(
        &mut doc,
        DocRange::new(DocPos::new(0, 1), DocPos::new(0, 4)),
        &RichCommand::ToggleBold,
    );
    // Expect runs: "a" | "bcd"(bold) | "ef"
    assert_eq!(doc.blocks[0].runs.len(), 3);
    assert_eq!(doc.blocks[0].runs[0].text, "a");
    assert!(!doc.blocks[0].runs[0].style.bold);
    assert_eq!(doc.blocks[0].runs[1].text, "bcd");
    assert!(doc.blocks[0].runs[1].style.bold);
    assert_eq!(doc.blocks[0].runs[2].text, "ef");
    assert!(!doc.blocks[0].runs[2].style.bold);
}

#[test]
fn cross_block_inline_command() {
    let mut doc = RichDoc::from_blocks(vec![Block::plain("hello"), Block::plain("world")]);
    apply_command(
        &mut doc,
        DocRange::new(DocPos::new(0, 3), DocPos::new(1, 2)),
        &RichCommand::ToggleItalic,
    );
    // Block 0: "hel" | "lo"(italic)
    assert_eq!(doc.blocks[0].runs.len(), 2);
    assert!(!doc.blocks[0].runs[0].style.italic);
    assert!(doc.blocks[0].runs[1].style.italic);
    // Block 1: "wo"(italic) | "rld"
    assert_eq!(doc.blocks[1].runs.len(), 2);
    assert!(doc.blocks[1].runs[0].style.italic);
    assert!(!doc.blocks[1].runs[1].style.italic);
}

#[test]
fn set_color_and_normalization() {
    let mut doc = doc_one("abc");
    let r = full_range(&doc, 0);
    let c = Color::from_rgb8(255, 0, 0);
    apply_command(&mut doc, r, &RichCommand::SetTextColor(c));
    assert_eq!(doc.blocks[0].runs.len(), 1);
    assert_eq!(doc.blocks[0].runs[0].style.text_color, Some(c));
}

#[test]
fn set_highlight_none_clears() {
    let mut doc = doc_one("abc");
    let r = full_range(&doc, 0);
    apply_command(
        &mut doc,
        r,
        &RichCommand::SetHighlight(Some(Color::from_rgb8(1, 2, 3))),
    );
    assert!(doc.blocks[0].runs[0].style.highlight.is_some());
    apply_command(&mut doc, r, &RichCommand::SetHighlight(None));
    assert!(doc.blocks[0].runs[0].style.highlight.is_none());
}

#[test]
fn set_align_over_blocks() {
    let mut doc = RichDoc::from_blocks(vec![Block::plain("a"), Block::plain("b")]);
    apply_command(
        &mut doc,
        DocRange::new(DocPos::new(0, 0), DocPos::new(1, 1)),
        &RichCommand::SetAlign(TextHAlign::Right),
    );
    assert_eq!(doc.blocks[0].align, TextHAlign::Right);
    assert_eq!(doc.blocks[1].align, TextHAlign::Right);
}

#[test]
fn set_list_toggles_off_when_all_same() {
    let mut doc = RichDoc::from_blocks(vec![Block::plain("a"), Block::plain("b")]);
    let r = DocRange::new(DocPos::new(0, 0), DocPos::new(1, 1));
    apply_command(&mut doc, r, &RichCommand::SetList(ListKind::Ordered));
    assert!(doc.blocks.iter().all(|b| b.list == ListKind::Ordered));
    // Applying the same kind again toggles it off.
    apply_command(&mut doc, r, &RichCommand::SetList(ListKind::Ordered));
    assert!(doc.blocks.iter().all(|b| b.list == ListKind::None));
}

#[test]
fn set_list_switches_kind_when_mixed() {
    let mut doc = RichDoc::from_blocks(vec![
        Block {
            list: ListKind::Bullet,
            ..Block::plain("a")
        },
        Block::plain("b"),
    ]);
    let r = DocRange::new(DocPos::new(0, 0), DocPos::new(1, 1));
    apply_command(&mut doc, r, &RichCommand::SetList(ListKind::Bullet));
    // Not all were Bullet -> set to Bullet (not toggle off).
    assert!(doc.blocks.iter().all(|b| b.list == ListKind::Bullet));
}

#[test]
fn indent_clamps_and_outdent_floors() {
    let mut doc = doc_one("x");
    let r = full_range(&doc, 0);
    for _ in 0..20 {
        apply_command(&mut doc, r, &RichCommand::Indent);
    }
    assert_eq!(doc.blocks[0].indent, MAX_INDENT);
    for _ in 0..20 {
        apply_command(&mut doc, r, &RichCommand::Outdent);
    }
    assert_eq!(doc.blocks[0].indent, 0);
}

#[test]
fn style_at_uses_char_before() {
    let mut doc = doc_one("ab");
    // Make "b" bold.
    apply_command(
        &mut doc,
        DocRange::new(DocPos::new(0, 1), DocPos::new(0, 2)),
        &RichCommand::ToggleBold,
    );
    // At byte 0 (start) -> first run style (not bold).
    assert!(!style_at(&doc, DocPos::new(0, 0)).bold);
    // At byte 1 -> char before is "a" (not bold).
    assert!(!style_at(&doc, DocPos::new(0, 1)).bold);
    // At byte 2 -> char before is "b" (bold).
    assert!(style_at(&doc, DocPos::new(0, 2)).bold);
}

#[test]
fn range_common_style_reports_mixed_as_none() {
    let mut doc = doc_one("abcd");
    // Bold only "ab".
    apply_command(
        &mut doc,
        DocRange::new(DocPos::new(0, 0), DocPos::new(0, 2)),
        &RichCommand::ToggleBold,
    );
    let cs = range_common_style(&doc, full_range(&doc, 0));
    // Mixed bold across the range.
    assert_eq!(cs.bold, None);
    // All share default (inherited) font family / color.
    assert_eq!(cs.font_family, Some(None));
    assert_eq!(cs.text_color, Some(None));
}

#[test]
fn range_common_style_all_agree() {
    let mut doc = doc_one("abcd");
    let r = full_range(&doc, 0);
    apply_command(&mut doc, r, &RichCommand::ToggleBold);
    let cs = range_common_style(&doc, r);
    assert_eq!(cs.bold, Some(true));
}

/// A fresh single block reports its concrete block align/list — the toolbar
/// reads these to decide which alignment/list toggle is active. A default
/// block is Left-aligned with no list, so exactly the Left button lights up
/// and neither list button does.
#[test]
fn common_style_reports_block_align_and_list() {
    let doc = doc_one("abcd");
    let cs = range_common_style(&doc, full_range(&doc, 0));
    assert_eq!(cs.align, Some(TextHAlign::Left));
    assert_eq!(cs.list, Some(ListKind::None));

    let mut doc = doc_one("abcd");
    let r = full_range(&doc, 0);
    apply_command(&mut doc, r, &RichCommand::SetAlign(TextHAlign::Center));
    apply_command(&mut doc, r, &RichCommand::SetList(ListKind::Ordered));
    let cs = range_common_style(&doc, r);
    assert_eq!(cs.align, Some(TextHAlign::Center));
    assert_eq!(cs.list, Some(ListKind::Ordered));
}

/// When a selection spans blocks whose align/list disagree, both collapse to
/// `None` (mixed) so the toolbar shows no alignment/list button as active.
#[test]
fn common_style_block_attrs_mixed_across_blocks() {
    let mut doc = RichDoc::from_blocks(vec![Block::plain("aaa"), Block::plain("bbb")]);
    // Give only the first block a Center align + ordered list.
    let block0 = full_range(&doc, 0);
    apply_command(&mut doc, block0, &RichCommand::SetAlign(TextHAlign::Center));
    apply_command(&mut doc, block0, &RichCommand::SetList(ListKind::Ordered));
    // Selection spanning both blocks now disagrees on align and list.
    let span = DocRange::new(DocPos::new(0, 0), DocPos::new(1, doc.blocks[1].text_len()));
    let cs = range_common_style(&doc, span);
    assert_eq!(cs.align, None, "mixed align must report None");
    assert_eq!(cs.list, None, "mixed list must report None");
}
