//! agg-sharp's `RichEditorKeyboardTests.cs`, ported 1:1 over
//! `RichEditorState` (the edit widget's state; C# sent the keys to
//! `RichMarkdownEditWidget`, which forwards them there).  C#'s 400 x 300
//! container is the state's viewport.  Windows bindings unless a test asks
//! for mac; Cmd is `meta` with mac bindings (C#'s mac layer reported it as
//! Control).

use std::sync::Arc;

use agg_gui::event::{Key, Modifiers};
use agg_gui::text::Font;
use agg_gui::widgets::markdown_editing::block_layout::RichBlockGeometry;
use agg_gui::widgets::markdown_editing::edit_operations::whole_block;
use agg_gui::widgets::markdown_editing::{
    DocPosition, InlineAtomKind, RichBlockKind, RichBlockLayout, RichCaret, RichEditorState,
    RichLayout, RichLayoutFonts, RichLayoutStyle, RichSelection,
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

fn make_sized(markdown: &str, width: f64, height: f64, mac: bool) -> RichEditorState {
    let mut editor = RichEditorState::new(Box::new(style));
    editor.use_mac_key_bindings = mac;
    editor.set_markdown(markdown).unwrap();
    editor.set_viewport(width, height).unwrap();
    editor
}

fn make(markdown: &str) -> RichEditorState {
    make_sized(markdown, 400.0, 300.0, false)
}

const NONE: Modifiers = Modifiers {
    shift: false,
    ctrl: false,
    alt: false,
    meta: false,
};
const SHIFT: Modifiers = Modifiers {
    shift: true,
    ..NONE
};
const CTRL: Modifiers = Modifiers { ctrl: true, ..NONE };
const CTRL_SHIFT: Modifiers = Modifiers {
    ctrl: true,
    shift: true,
    ..NONE
};
const ALT: Modifiers = Modifiers { alt: true, ..NONE };
const META: Modifiers = Modifiers { meta: true, ..NONE };

fn type_text(editor: &mut RichEditorState, text: &str) {
    for c in text.chars() {
        editor.key_down(&Key::Char(c), NONE).unwrap();
    }
}

fn key(editor: &mut RichEditorState, key: Key, modifiers: Modifiers) -> bool {
    editor.key_down(&key, modifiers).unwrap()
}

fn caret(editor: &mut RichEditorState, block: usize, offset: usize) {
    editor.select(RichSelection::at(DocPosition::new(block, offset)));
}

fn text_layout(editor: &RichEditorState, block: usize) -> &RichBlockLayout {
    match editor.view().layout_of(block) {
        Some(RichLayout::Text(layout)) => layout,
        other => panic!("block {block} has no text layout: {other:?}"),
    }
}

#[test]
fn typing_a_sentence_and_enter_splits() {
    let mut editor = make("");
    type_text(&mut editor, "Hello world");
    key(&mut editor, Key::Enter, NONE);
    type_text(&mut editor, "Next");
    assert_eq!(editor.markdown(), "Hello world\n\nNext");
    assert_eq!(editor.selection().caret, DocPosition::new(1, 4));
}

#[test]
fn hash_space_makes_a_heading_and_one_undo_keeps_the_typed_marker() {
    let mut editor = make("");
    type_text(&mut editor, "# ");
    assert_eq!(editor.document().blocks[0].kind, RichBlockKind::Heading);
    type_text(&mut editor, "Title");
    assert_eq!(editor.markdown(), "# Title");

    key(&mut editor, Key::Char('z'), CTRL);
    key(&mut editor, Key::Char('z'), CTRL);
    assert_eq!(editor.markdown(), "\\# ");
    assert_eq!(editor.document().blocks[0].kind, RichBlockKind::Paragraph);

    key(&mut editor, Key::Char('z'), CTRL_SHIFT);
    assert_eq!(editor.document().blocks[0].kind, RichBlockKind::Heading);
}

#[test]
fn backspace_and_delete_join_blocks() {
    let mut editor = make("one\n\ntwo\n");
    caret(&mut editor, 1, 0);
    key(&mut editor, Key::Backspace, NONE);
    assert_eq!(editor.markdown(), "onetwo\n");
    assert_eq!(editor.selection().caret, DocPosition::new(0, 3));

    key(&mut editor, Key::Delete, NONE);
    assert_eq!(editor.markdown(), "onewo\n");

    // Backspace at the document's start changes nothing and leaves no empty undo step.
    let steps = editor.history().undo_count();
    caret(&mut editor, 0, 0);
    key(&mut editor, Key::Backspace, NONE);
    assert_eq!(editor.history().undo_count(), steps);
}

#[test]
fn delete_removes_a_selected_raw_block_whole() {
    let mut editor = make("before\n\n<table><tr><td>x</td></tr></table>\n\nafter\n");
    let raw = editor
        .document()
        .blocks
        .iter()
        .position(|b| b.kind == RichBlockKind::Raw);
    assert_eq!(raw, Some(1));

    let whole = whole_block(editor.document(), 1);
    editor.select(whole);
    key(&mut editor, Key::Delete, NONE);
    assert!(!editor
        .document()
        .blocks
        .iter()
        .any(|b| b.kind == RichBlockKind::Raw));
    assert_eq!(editor.document().blocks.len(), 2);
}

#[test]
fn word_backspace_deletes_a_word() {
    let mut editor = make("hello world");
    caret(&mut editor, 0, 11);
    key(&mut editor, Key::Backspace, CTRL);
    assert_eq!(editor.markdown(), "hello ");
}

#[test]
fn bold_over_a_selection_and_at_a_caret() {
    let mut editor = make("make bold");
    editor.select(RichSelection::new(
        DocPosition::new(0, 5),
        DocPosition::new(0, 9),
    ));
    key(&mut editor, Key::Char('b'), CTRL);
    assert_eq!(editor.markdown(), "make **bold**");

    let mut plain = make("a");
    caret(&mut plain, 0, 1);
    key(&mut plain, Key::Char('b'), CTRL);
    type_text(&mut plain, "bc");
    assert_eq!(plain.markdown(), "a**bc**");

    // The pending style goes when the caret moves.
    key(&mut plain, Key::Char('i'), CTRL);
    key(&mut plain, Key::ArrowLeft, NONE);
    assert!(plain.pending_style().is_empty());
}

#[test]
fn arrows_step_over_an_emoji_whole() {
    // Offsets count chars here, where C# counted UTF-16 units: the emoji is
    // one offset (C#'s 1 -> 3 is 1 -> 2), and still one step.
    let mut editor = make("a\u{1F600}b");
    caret(&mut editor, 0, 1);
    key(&mut editor, Key::ArrowRight, NONE);
    assert_eq!(editor.selection().caret.offset, 2);
    key(&mut editor, Key::ArrowLeft, NONE);
    assert_eq!(editor.selection().caret.offset, 1);

    // Right at a block's end goes to the next block's start.
    let mut two = make("a\n\nb");
    caret(&mut two, 0, 1);
    key(&mut two, Key::ArrowRight, NONE);
    assert_eq!(two.selection().caret, DocPosition::new(1, 0));
}

#[test]
fn up_and_down_keep_the_column_across_a_short_line() {
    let mut editor = make("same words on this line\n\nab\n\nsame words on this line\n");
    caret(&mut editor, 0, 15);
    key(&mut editor, Key::ArrowDown, NONE);
    assert_eq!(editor.selection().caret, DocPosition::new(1, 2));
    key(&mut editor, Key::ArrowDown, NONE);
    assert_eq!(editor.selection().caret, DocPosition::new(2, 15));
    key(&mut editor, Key::ArrowUp, NONE);
    key(&mut editor, Key::ArrowUp, NONE);
    assert_eq!(editor.selection().caret, DocPosition::new(0, 15));
}

#[test]
fn up_and_down_move_through_a_wrapped_paragraph() {
    let mut editor = make_sized(&vec!["word"; 40].join(" "), 200.0, 300.0, false);
    let lines = text_layout(&editor, 0).lines.clone();
    assert!(lines.len() > 2);

    caret(&mut editor, 0, 2);
    key(&mut editor, Key::ArrowDown, NONE);
    let offset = editor.selection().caret.offset;
    assert!(offset >= lines[1].start);
    assert!(offset < lines[2].start);
    key(&mut editor, Key::ArrowUp, NONE);
    assert_eq!(editor.selection().caret.offset, 2);
}

#[test]
fn home_and_end_go_to_the_drawn_lines_ends() {
    let mut editor = make_sized(&vec!["word"; 40].join(" "), 200.0, 300.0, false);
    let line = text_layout(&editor, 0).lines[1].clone();
    caret(&mut editor, 0, line.start + 2);
    key(&mut editor, Key::Home, NONE);
    assert_eq!(editor.selection().caret.offset, line.start);
    key(&mut editor, Key::End, NONE);
    let end = editor.selection().caret.offset;
    assert!(end > line.start + 2);
    assert!(end <= line.end);

    // Shift extends from where the caret was.
    key(&mut editor, Key::Home, SHIFT);
    assert_eq!(editor.selection().anchor.offset, end);
    assert_eq!(editor.selection().caret.offset, line.start);
}

#[test]
fn shift_arrow_selects_and_mac_command_arrow_goes_to_the_line_end() {
    let mut editor = make_sized("abc def", 400.0, 300.0, true);
    caret(&mut editor, 0, 0);
    key(&mut editor, Key::ArrowRight, SHIFT);
    key(&mut editor, Key::ArrowRight, SHIFT);
    assert_eq!(
        editor.selection(),
        RichSelection::new(DocPosition::new(0, 0), DocPosition::new(0, 2))
    );

    // On mac Option steps words and Command goes to the line's end.
    caret(&mut editor, 0, 0);
    key(&mut editor, Key::ArrowRight, ALT);
    assert_eq!(editor.selection().caret.offset, 4);
    key(&mut editor, Key::ArrowRight, META);
    assert_eq!(editor.selection().caret.offset, 7);
}

#[test]
fn select_all_then_typing_replaces_everything() {
    let mut editor = make("# Title\n\nbody text\n");
    key(&mut editor, Key::Char('a'), CTRL);
    type_text(&mut editor, "x");
    assert_eq!(editor.document().blocks.len(), 1);
    assert_eq!(editor.document().blocks[0].text_length(0, 0), 1);
}

#[test]
fn tab_moves_table_cells_and_indents_list_items() {
    let mut table = make("| a | b |\n|---|---|\n| c | d |\n");
    caret(&mut table, 0, 0);
    assert!(key(&mut table, Key::Tab, NONE));
    assert_eq!(table.selection().caret, DocPosition::in_cell(0, 1, 0, 1));
    key(&mut table, Key::Tab, SHIFT);
    assert_eq!(table.selection().caret, DocPosition::in_cell(0, 1, 0, 0));

    let mut list = make("- one\n- two\n");
    caret(&mut list, 1, 0);
    key(&mut list, Key::Tab, NONE);
    assert_eq!(list.document().blocks[1].list.as_ref().unwrap().depth, 1);

    // In a paragraph Tab is left for focus traversal.
    let mut paragraph = make("text");
    caret(&mut paragraph, 0, 0);
    assert!(!key(&mut paragraph, Key::Tab, NONE));
    assert_eq!(paragraph.markdown(), "text");
}

#[test]
fn undo_and_redo_restore_text_and_caret() {
    let mut editor = make("start");
    caret(&mut editor, 0, 5);
    // " more" is two undo steps, as in most editors: the space, then the word.
    type_text(&mut editor, " more");
    key(&mut editor, Key::Char('z'), CTRL);
    key(&mut editor, Key::Char('z'), CTRL);
    assert_eq!(editor.markdown(), "start");
    assert_eq!(editor.selection().caret, DocPosition::new(0, 5));

    key(&mut editor, Key::Char('y'), CTRL);
    key(&mut editor, Key::Char('y'), CTRL);
    assert_eq!(editor.markdown(), "start more");
    assert_eq!(editor.selection().caret, DocPosition::new(0, 10));
}

#[test]
fn list_children_lifted_by_an_edit_are_laid_out_at_their_new_depth() {
    // Backspace at the parent's start takes it out of the list; its children lift a level without the
    // edit naming them, and must not keep their indented layouts.
    let mut editor = make("- a\n  - b\n  - c\n\npara\n");
    let child_indent = text_layout(&editor, 1).text_left;
    let fresh = make("- b\n- c\n");
    let top_indent = text_layout(&fresh, 0).text_left;
    assert!(child_indent > top_indent);

    caret(&mut editor, 0, 0);
    key(&mut editor, Key::Backspace, NONE);
    assert_eq!(editor.document().blocks[1].list.as_ref().unwrap().depth, 0);
    assert_eq!(text_layout(&editor, 1).text_left, top_indent);
    assert_eq!(text_layout(&editor, 2).text_left, top_indent);

    // Tab on an item with a subtree moves the subtree too.
    let mut tree = make("- a\n- b\n  - c\n");
    caret(&mut tree, 1, 0);
    key(&mut tree, Key::Tab, NONE);
    assert_eq!(tree.document().blocks[2].list.as_ref().unwrap().depth, 2);
    assert!(text_layout(&tree, 2).text_left > text_layout(&tree, 1).text_left);
}

#[test]
fn a_merging_backspace_lays_out_only_the_merged_block() {
    let text: String = (0..30).map(|i| format!("Paragraph {i}.\n\n")).collect();
    let mut editor = make(&text);
    let before = editor.view().layout_count();
    caret(&mut editor, 10, 0);
    key(&mut editor, Key::Backspace, NONE);
    assert_eq!(editor.document().blocks.len(), 29);
    assert!(editor.view().layout_count() - before <= 2);
}

#[test]
fn shift_enter_at_a_blocks_end_saves_no_stray_backslash() {
    let mut editor = make("line\n");
    caret(&mut editor, 0, 4);
    key(&mut editor, Key::Enter, SHIFT);
    assert!(!editor.markdown().contains('\\'));

    type_text(&mut editor, "next");
    let markdown = editor.markdown();
    let reloaded = make(&markdown);
    assert_eq!(reloaded.document().blocks.len(), 1);
    assert_eq!(reloaded.markdown(), markdown);
    let breaks: Vec<_> = reloaded.document().blocks[0]
        .inlines
        .iter()
        .filter_map(|i| i.as_atom())
        .collect();
    assert_eq!(breaks.len(), 1);
    assert_eq!(breaks[0].kind, InlineAtomKind::HardBreak);
}

#[test]
fn the_caret_is_scrolled_into_view() {
    let text: String = (0..40)
        .map(|i| format!("Paragraph number {i} with a few words.\n\n"))
        .collect();
    let mut editor = make(&text);
    key(&mut editor, Key::End, CTRL);
    assert!(editor.scroll_offset_from_top() > 0.0);
    let visible = editor.visible_bounds();
    let last = editor.document().blocks.len() - 1;
    let caret_width = editor.style().unwrap().caret_width();
    let layout = editor.view().layout_of(last).unwrap();
    let rect = layout
        .caret_rect(RichCaret::new(editor.selection().caret.offset), caret_width)
        .unwrap();
    let origin = editor.view().origin_of(last).unwrap();
    assert!(rect.y + origin >= visible.y - 0.001);
    assert!(rect.y + rect.height + origin <= visible.y + visible.height + 0.001);

    key(&mut editor, Key::Home, CTRL);
    assert_eq!(editor.scroll_offset_from_top(), 0.0);
}
