//! `rust_only_*` tests for `markdown_editing::document_view`,
//! `caret_navigator` and `caret_blink` (agg-sharp's `RichDocumentView`,
//! `RichCaretNavigator`, `RichCaretBlink`).  agg-sharp exercises these through
//! the edit widget (`RichMarkdownEditWidgetTests`, `RichEditorKeyboardTests`);
//! these drive them over a bare document, and pin the C# crash the navigator
//! port fixes.

use std::sync::Arc;
use std::time::Duration;

use agg_gui::clock;
use agg_gui::text::Font;
use agg_gui::widgets::markdown_editing::caret_blink::RichCaretBlink;
use agg_gui::widgets::markdown_editing::caret_navigator as nav;
use agg_gui::widgets::markdown_editing::document_view::{
    ends_list, image_display_size, list_numbers,
};
use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichBlock, RichBlockKind, RichDocument, RichDocumentView, RichEditError,
    RichInline, RichLayoutFonts, RichLayoutStyle, RichTableCell, RichViewSync,
};

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");

fn style() -> RichLayoutStyle {
    RichLayoutStyle::new(
        RichLayoutFonts {
            regular: Arc::new(Font::from_slice(REGULAR).expect("demo font")),
            bold: None,
            mono: None,
        },
        Some(1.0),
    )
}

fn laid_out(document: &RichDocument, width: f64) -> RichDocumentView {
    let mut view = RichDocumentView::new();
    assert!(view.set_width(width));
    relayout(&mut view, document, None);
    view
}

fn relayout(
    view: &mut RichDocumentView,
    document: &RichDocument,
    changed: Option<&[agg_gui::widgets::markdown_editing::RichBlockId]>,
) {
    let mut build = style;
    let sync = RichViewSync {
        document,
        scroll_top: None,
        device_scale: 1.0,
        typography_epoch: 0,
        build_style: &mut build,
    };
    match changed {
        Some(changed) => view.relayout(sync, changed).unwrap(),
        None => view.relayout_all(sync).unwrap(),
    };
}

#[test]
fn rust_only_relayout_reuses_unchanged_blocks_and_stacks_them() {
    let mut document = parse("First\n\nSecond\n\nThird\n");
    let mut view = laid_out(&document, 300.0);
    assert_eq!(view.layout_count(), 3);
    assert_eq!(view.laid_out_count(), 3);
    let first = view.block_bounds(0).unwrap();
    let second = view.block_bounds(1).unwrap();
    assert!((first.y - (second.y + second.height)).abs() < 1e-9);

    document.blocks[1].inlines = vec![RichInline::run("Second, edited")];
    let id = document.blocks[1].id();
    relayout(&mut view, &document, Some(&[id]));
    assert_eq!(view.layout_count(), 4, "only the edited block re-lays out");

    // A clone is a new block: it is laid out, the original keeps its layout.
    let copy = document.blocks[0].clone();
    assert_ne!(copy.id(), document.blocks[0].id());
    document.blocks.insert(1, copy);
    relayout(&mut view, &document, Some(&[]));
    assert_eq!(view.layout_count(), 5);
}

#[test]
fn rust_only_nested_list_of_the_other_kind_does_not_end_the_list() {
    // Only a change of kind at the same depth starts another list.
    let blocks = parse("1. a\n   - b\n2. c\n").blocks;
    assert!(!ends_list(&blocks, 0));
    assert!(!ends_list(&blocks, 1));
    assert!(ends_list(&blocks, 2));
}

#[test]
fn rust_only_list_numbers_count_per_depth_and_restart() {
    let document = parse("3. a\n4. b\n   1. c\n   2. d\n5. e\n\nAfter\n");
    assert_eq!(list_numbers(&document.blocks), vec![3, 4, 1, 2, 5, 1]);
}

#[test]
fn rust_only_image_display_size_fits_the_column() {
    let size = image_display_size(Some((800, 400)), 200.0, 12.0);
    assert_eq!((size.width, size.height), (200.0, 100.0));
    let size = image_display_size(None, 200.0, 12.0);
    assert_eq!((size.width, size.height), (12.0, 12.0));
}

#[test]
fn rust_only_caret_steps_through_stops_and_blocks() {
    let document = parse("ab\n\ncd\n");
    let view = laid_out(&document, 300.0);
    let p = |b, o| DocPosition::new(b, o);

    let right = nav::next(&view, &document, p(0, 2)).unwrap().unwrap();
    assert_eq!(right.position, p(1, 0));
    let left = nav::previous(&view, &document, p(1, 0)).unwrap().unwrap();
    assert_eq!(left.position, p(0, 2));
    assert_eq!(nav::previous(&view, &document, p(0, 0)).unwrap(), None);
    let end = nav::line_edge(&view, &document, p(1, 0), false, true).unwrap();
    assert_eq!(end.position, p(1, 2));
    let down = nav::vertical(&view, &document, p(0, 1), false, 0.0, 1)
        .unwrap()
        .unwrap();
    assert_eq!(down.position.block_index, 1);
    let past = nav::vertical(&view, &document, p(1, 1), false, 0.0, 1)
        .unwrap()
        .unwrap();
    assert_eq!(past.position, p(1, 2), "past the last line goes to the end");
    assert!(nav::caret_bounds(&view, &document, p(0, 1), false)
        .unwrap()
        .is_some());
    assert_eq!(
        nav::next(&view, &document, p(5, 0)),
        Err(RichEditError::PositionOutOfRange(p(5, 0)))
    );
}

#[test]
fn rust_only_word_steps_without_layout() {
    let document = parse("one two\n");
    let next = nav::next_word(&document, DocPosition::new(0, 0))
        .unwrap()
        .unwrap();
    assert_eq!(next.position.offset, 4);
    let back = nav::previous_word(&document, DocPosition::new(0, 7))
        .unwrap()
        .unwrap();
    assert_eq!(back.position.offset, 4);
}

#[test]
fn rust_only_empty_table_row_is_stepped_over() {
    // C# indexed column -1 of a row with no cells and threw.
    let mut table = RichBlock::new(RichBlockKind::Table);
    table.table_rows = vec![vec![], vec![RichTableCell::default()]];
    let mut document = RichDocument::new();
    document.blocks = vec![RichBlock::new(RichBlockKind::Paragraph), table];
    let left = nav::previous_word(&document, DocPosition::in_cell(1, 0, 1, 0))
        .unwrap()
        .unwrap();
    assert_eq!(left.position, DocPosition::new(0, 0));

    let view = laid_out(&document, 300.0);
    let up = nav::vertical(
        &view,
        &document,
        DocPosition::in_cell(1, 0, 1, 0),
        false,
        0.0,
        -1,
    )
    .unwrap()
    .unwrap();
    assert_eq!(up.position.block_index, 0);
}

#[test]
fn rust_only_caret_blink_runs_on_the_ui_clock() {
    let _clock = clock::scoped_virtual(None);
    let mut blink = RichCaretBlink::new();
    assert!(blink.showing(), "a stopped blink shows the caret");
    assert!(!blink.restart(false));
    assert!(!blink.is_running());

    assert!(blink.restart(true));
    assert!(blink.showing());
    assert_eq!(blink.until_edge(), Some(Duration::from_millis(600)));
    clock::advance(Duration::from_millis(599));
    assert!(blink.showing());
    clock::advance(Duration::from_millis(1));
    assert!(!blink.showing());
    assert_eq!(blink.until_edge(), Some(Duration::from_millis(600)));
    clock::advance(Duration::from_millis(600));
    assert!(blink.showing());

    clock::advance(Duration::from_millis(700));
    assert!(blink.restart(true));
    assert!(blink.showing(), "a caret move restarts it showing");
    blink.tick(false);
    assert!(!blink.is_running());
}
