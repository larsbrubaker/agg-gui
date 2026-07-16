//! Unit tests for [`RichTextToolbar`](super::RichTextToolbar): the control
//! roster produced per config, command dispatch through a real handle (clicking
//! Bold bolds the selection), the tri-state active-display data source, and the
//! family-omitted configuration.

use std::sync::Arc;

use crate::event::{Event, Modifiers, MouseButton};
use crate::geometry::{Point, Size};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::rich_text::model::{Block, InlineStyle, RichDoc, TextRun};
use crate::widgets::rich_text::view::SharedResolver;
use crate::widgets::rich_text::{RichEditHandle, RichTextEdit};

use super::RichTextToolbar;

const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("test font loads"))
}

fn resolver(font: &Arc<Font>) -> SharedResolver {
    let f = Arc::clone(font);
    std::rc::Rc::new(move |_: &InlineStyle| Arc::clone(&f))
}

/// Build a toolbar over a fresh editor seeded with `doc`, returning both so the
/// test can drive the shared core through the returned handle.
fn toolbar_over(doc: RichDoc) -> (RichTextToolbar, RichEditHandle) {
    let font = font();
    let editor = RichTextEdit::new(doc, resolver(&font));
    let handle = editor.handle();
    (RichTextToolbar::new(handle.clone(), font), handle)
}

fn child_type_names(w: &dyn Widget) -> Vec<&'static str> {
    w.children().iter().map(|c| c.type_name()).collect()
}

/// The two-row `FlexColumn` root and its rows.
fn rows(toolbar: &RichTextToolbar) -> Vec<&dyn Widget> {
    let col = toolbar.children()[0].as_ref();
    col.children().iter().map(|c| c.as_ref()).collect()
}

// ── Control roster per config ──────────────────────────────────────────────

/// A fully-enabled toolbar with an injected family list lays out the complete
/// two-row roster: row 1 = B/I/U/S + family combo + size combo + text/highlight
/// swatches; row 2 = 3 alignments + 2 lists + outdent/indent + undo/redo.
#[test]
fn full_roster_with_families() {
    let (toolbar, _h) = toolbar_over(RichDoc::from_blocks(vec![Block::plain("hi")]));
    let toolbar = toolbar.with_families(vec!["Sans".into(), "Serif".into()], None);

    let rows = rows(&toolbar);
    assert_eq!(rows.len(), 2, "two rows");
    assert_eq!(
        child_type_names(rows[0]),
        ["Button", "Button", "Button", "Button", "ComboBox", "ComboBox", "Button", "Button"],
        "row 1: B/I/U/S, family combo, size combo, text-colour, highlight"
    );
    assert_eq!(child_type_names(rows[1]), vec!["Button"; 9], "row 2 roster");
}

/// Disabling groups drops exactly those controls and omits a fully-empty row.
#[test]
fn config_disables_controls_and_empty_rows() {
    let (toolbar, _h) = toolbar_over(RichDoc::from_blocks(vec![Block::plain("hi")]));
    let toolbar = toolbar
        .with_bold(false)
        .with_colors(false)
        .with_history(false)
        .with_alignment(false)
        .with_lists(false)
        .with_indent(false);

    let rows = rows(&toolbar);
    // Row 2's whole roster (align/lists/indent/history) is disabled, so the row
    // is omitted entirely — only row 1 survives.
    assert_eq!(rows.len(), 1, "empty second row omitted");
    // Row 1: italic, underline, strike (bold off), then size combo (colours off,
    // no families).
    assert_eq!(
        child_type_names(rows[0]),
        ["Button", "Button", "Button", "ComboBox"],
        "row 1 lost bold + colours, kept I/U/S + size combo"
    );
}

/// With no injected family list the family dropdown is omitted: row 1 carries a
/// single `ComboBox` (the size dropdown), not two.
#[test]
fn family_omitted_when_no_families() {
    let (toolbar, _h) = toolbar_over(RichDoc::from_blocks(vec![Block::plain("hi")]));
    let rows = rows(&toolbar);
    assert_eq!(
        child_type_names(rows[0]),
        ["Button", "Button", "Button", "Button", "ComboBox", "Button", "Button"],
        "row 1 without a family combo: B/I/U/S, size combo, two colour swatches"
    );
    let combo_count = child_type_names(rows[0]).iter().filter(|t| **t == "ComboBox").count();
    assert_eq!(combo_count, 1, "only the size ComboBox, no family ComboBox");
}

// ── Command dispatch through a real handle ─────────────────────────────────

/// Reach the Bold toggle: root FlexColumn → row 1 → first button.
fn bold_button(toolbar: &mut RichTextToolbar) -> &mut Box<dyn Widget> {
    let col = &mut toolbar.children_mut()[0];
    let row1 = &mut col.children_mut()[0];
    &mut row1.children_mut()[0]
}

/// A real click on the Bold toggle must dispatch `ToggleBold` through the shared
/// handle and bold the selected run — proving the toolbar drives the very editor
/// it was built over.
#[test]
fn click_bold_bolds_selection_through_handle() {
    let (mut toolbar, handle) = toolbar_over(RichDoc::from_blocks(vec![Block::plain("hello")]));
    // A real selection so the toggle mutates the document (not just pending).
    handle.select_all();
    assert_ne!(handle.common_style_of_selection().bold, Some(true), "not bold yet");

    // Lay out so the button has non-zero bounds for its local hit-test.
    toolbar.layout(Size::new(600.0, 100.0));

    let button = bold_button(&mut toolbar);
    let b = button.bounds();
    let pos = Point::new(b.width * 0.5, b.height * 0.5);
    let down = Event::MouseDown { pos, button: MouseButton::Left, modifiers: Modifiers::default() };
    let up = Event::MouseUp { pos, button: MouseButton::Left, modifiers: Modifiers::default() };
    button.on_event(&down);
    button.on_event(&up);

    assert_eq!(
        handle.common_style_of_selection().bold,
        Some(true),
        "clicking Bold bolded the whole selection through the handle"
    );
    assert_eq!(handle.plain_text(), "hello", "text unchanged by formatting");
}

// ── Tri-state active display ───────────────────────────────────────────────

/// The toggle's active state is `common_style_of_selection().<attr> == Some(true)`.
/// This exercises the production data source the toolbar reads each frame: a
/// uniform-bold selection reports `Some(true)` (active), a mixed one `None`
/// (inactive), and an all-plain one `Some(false)` (inactive).
#[test]
fn bold_toggle_reflects_tri_state() {
    let bold = InlineStyle { bold: true, ..Default::default() };

    // Uniform bold selection → active.
    let (toolbar, handle) = toolbar_over(RichDoc::from_blocks(vec![Block::from_run(
        TextRun::new("bold", bold.clone()),
    )]));
    handle.select_all();
    let cs = toolbar.common_style_of_selection();
    assert_eq!(cs.bold, Some(true));
    assert!(cs.bold == Some(true), "uniform bold ⇒ toggle active");

    // Mixed selection (bold + plain) → inactive (tri-state "mixed").
    let (toolbar, handle) = toolbar_over(RichDoc::from_blocks(vec![Block {
        runs: vec![TextRun::new("B", bold), TextRun::plain("p")],
        ..Block::new()
    }]));
    handle.select_all();
    let cs = toolbar.common_style_of_selection();
    assert_eq!(cs.bold, None, "mixed selection reports None");
    assert!(cs.bold != Some(true), "mixed ⇒ toggle inactive");

    // All-plain selection → inactive.
    let (toolbar, handle) = toolbar_over(RichDoc::from_blocks(vec![Block::plain("plain")]));
    handle.select_all();
    assert_eq!(toolbar.common_style_of_selection().bold, Some(false));
}
