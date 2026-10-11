//! agg-sharp's `RichEditorClipboardTests.cs`, ported 1:1 over
//! `RichEditorState` on agg-gui's simulated clipboard
//! (`clipboard::simulate`, C#'s `SimulatedClipboard`).

use std::sync::Arc;

use agg_gui::clipboard;
use agg_gui::event::{Key, Modifiers};
use agg_gui::text::Font;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichBlockKind, RichEditorState, RichLayoutFonts, RichLayoutStyle, RichSelection,
    RichThemeColors,
};
use agg_gui::Color;

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");

fn fonts() -> RichLayoutFonts {
    RichLayoutFonts {
        regular: Arc::new(Font::from_slice(REGULAR).expect("demo font")),
        bold: None,
        mono: None,
    }
}

fn style() -> RichLayoutStyle {
    RichLayoutStyle::new(fonts(), Some(1.0))
}

fn make(markdown: &str) -> (RichEditorState, clipboard::SimulatedClipboard) {
    make_styled(markdown, Box::new(style))
}

fn make_styled(
    markdown: &str,
    build_style: Box<dyn FnMut() -> RichLayoutStyle>,
) -> (RichEditorState, clipboard::SimulatedClipboard) {
    let simulated = clipboard::simulate();
    let mut editor = RichEditorState::new(build_style);
    editor.use_mac_key_bindings = false;
    editor.set_markdown(markdown).unwrap();
    editor.set_viewport(400.0, 300.0).unwrap();
    (editor, simulated)
}

fn key(editor: &mut RichEditorState, key: Key, modifiers: Modifiers) {
    editor.key_down(&key, modifiers).unwrap();
}

fn ctrl(editor: &mut RichEditorState, letter: char) {
    let modifiers = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    key(editor, Key::Char(letter), modifiers);
}

fn select(editor: &mut RichEditorState, block: usize, from: usize, to: usize) {
    editor.select(RichSelection::new(
        DocPosition::new(block, from),
        DocPosition::new(block, to),
    ));
}

#[test]
fn copy_then_paste_keeps_bold_and_link() {
    let (mut editor, _clipboard) = make("A **bold** [link](https://x.com) end\n");
    select(&mut editor, 0, 2, 11);
    ctrl(&mut editor, 'c');
    assert_eq!(clipboard::get_text().as_deref(), Some("bold link"));
    let html = clipboard::get_html().unwrap_or_default();
    assert!(html.contains("<strong"), "{html}");
    assert!(html.contains("href=\"https://x.com\""), "{html}");

    editor.select(RichSelection::at(DocPosition::new(0, 15)));
    ctrl(&mut editor, 'v');
    assert_eq!(
        editor.markdown(),
        "A **bold** [link](https://x.com) end**bold** [link](https://x.com)\n"
    );
}

#[test]
fn cut_is_one_undo_step() {
    let (mut editor, _clipboard) = make("Hello world\n");
    select(&mut editor, 0, 5, 11);
    ctrl(&mut editor, 'x');
    assert_eq!(editor.markdown(), "Hello\n");
    assert_eq!(clipboard::get_text().as_deref(), Some(" world"));

    ctrl(&mut editor, 'z');
    assert_eq!(editor.markdown(), "Hello world\n");
}

#[test]
fn markdown_text_from_elsewhere_pastes_formatted() {
    let (mut editor, _clipboard) = make("Start\n");
    clipboard::set_text("**bold** and\n\n- item");
    editor.select(RichSelection::at(DocPosition::new(0, 5)));
    ctrl(&mut editor, 'v');
    assert_eq!(editor.markdown(), "Start**bold** and\n\n- item\n");
    assert_eq!(editor.document().blocks[1].kind, RichBlockKind::ListItem);

    // The paste is one undo step of its own.
    ctrl(&mut editor, 'z');
    assert_eq!(editor.markdown(), "Start\n");
}

#[test]
fn plain_line_takes_the_caret_style_and_replaces_the_selection() {
    let (mut editor, _clipboard) = make("**Hello world**\n");
    clipboard::set_text("there");
    select(&mut editor, 0, 6, 11);
    ctrl(&mut editor, 'v');
    assert_eq!(editor.markdown(), "**Hello there**\n");
}

#[test]
fn code_block_gets_plain_text() {
    let (mut editor, _clipboard) = make("```\nab\n```\n");
    clipboard::set_text("**x**");
    editor.select(RichSelection::at(DocPosition::new(0, 1)));
    ctrl(&mut editor, 'v');
    assert_eq!(editor.markdown(), "```\na**x**b\n```\n");
}

#[test]
fn copy_in_one_editor_pastes_formatted_in_another() {
    let (mut first, first_clipboard) = make("# Head line\n\nSome text\n");
    first.select(RichSelection::new(
        DocPosition::new(0, 5),
        DocPosition::new(1, 4),
    ));
    ctrl(&mut first, 'c');

    // A second editor in the same process; make swaps in a new clipboard, so carry the text over.
    let text = clipboard::get_text().unwrap_or_default();
    drop(first_clipboard);
    let (mut second, _clipboard) = make("");
    clipboard::set_text(&text);
    ctrl(&mut second, 'v');
    assert_eq!(second.markdown(), "# line\n\nSome");
}

#[test]
fn crlf_copy_is_still_recognised_on_paste() {
    let (mut editor, editor_clipboard) = make("# Title\r\n\r\n<div>\r\nhi\r\n</div>\r\n");
    ctrl(&mut editor, 'a');
    ctrl(&mut editor, 'c');
    let text = clipboard::get_text().unwrap_or_default();
    drop(editor_clipboard);
    let (mut other, _clipboard) = make("");
    clipboard::set_text(&text);
    ctrl(&mut other, 'v');
    assert!(other.markdown().starts_with("# Title"));
}

#[test]
fn shift_down_copy_pastes_no_empty_item() {
    let (mut editor, _clipboard) = make("- a\n- b\n\nEnd\n");
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    key(&mut editor, Key::ArrowDown, shift);
    assert_eq!(editor.selection().end(), DocPosition::new(1, 0));
    ctrl(&mut editor, 'c');
    editor.select(RichSelection::at(DocPosition::new(2, 3)));
    ctrl(&mut editor, 'v');
    assert_eq!(editor.markdown(), "- a\n- b\n\nEnd\n\n- a\n");
}

#[test]
fn dark_theme_copy_pastes_with_a_light_background() {
    let dark = RichThemeColors {
        background: Color::from_rgb8(0x10, 0x10, 0x10),
        text: Color::from_rgb8(0xEE, 0xEE, 0xEE),
        accent: Color::from_rgb8(0x7A, 0xD7, 0xF0),
    };
    let (mut editor, _clipboard) = make_styled(
        "Some text\n",
        Box::new(move || RichLayoutStyle::with_theme(fonts(), Some(&dark), Some(1.0))),
    );
    ctrl(&mut editor, 'a');
    ctrl(&mut editor, 'c');
    let html = clipboard::get_html().unwrap_or_default().to_lowercase();
    assert!(!html.is_empty(), "the copy put HTML on the clipboard");
    assert!(!html.contains("101010"), "{html}");
    assert!(!html.contains("eeeeee"), "{html}");
}

#[test]
fn host_handles_paste_first() {
    let (mut editor, _clipboard) = make("Text\n");
    clipboard::set_text("more");
    editor.on_paste_requested(Box::new(|| true));
    editor.select(RichSelection::at(DocPosition::new(0, 4)));
    ctrl(&mut editor, 'v');
    assert_eq!(editor.markdown(), "Text\n");
}
