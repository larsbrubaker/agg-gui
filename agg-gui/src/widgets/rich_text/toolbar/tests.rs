//! Unit tests for [`RichTextToolbar`](super::RichTextToolbar): the control
//! roster produced per config, command dispatch through a real handle (clicking
//! Bold bolds the selection), the tri-state active-display data source, and the
//! family-omitted configuration.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, Modifiers, MouseButton};
use crate::geometry::{Point, Size};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::rich_text::commands::RichCommand;
use crate::widgets::rich_text::model::{Block, InlineStyle, RichDoc, TextRun};
use crate::widgets::rich_text::view::SharedResolver;
use crate::widgets::rich_text::{RichEditHandle, RichTextEdit};

use super::RichTextToolbar;

/// Font Awesome "eraser" glyph the Remove-highlight button renders.
const ICON_ERASER: &str = "\u{F12D}";

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
        ["Button", "Button", "Button", "Button", "ComboBox", "ComboBox", "Button", "Button", "Button"],
        "row 1: B/I/U/S, family combo, size combo, text-colour, highlight, remove-highlight"
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
        ["Button", "Button", "Button", "Button", "ComboBox", "Button", "Button", "Button"],
        "row 1 without a family combo: B/I/U/S, size combo, two colour swatches, remove-highlight"
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

// ── Colour picker: "No Color" checkbox is OFF in the rich-text host ────────

/// Recursively collect every descendant `type_name` under `w` (inclusive of
/// the built dialog subtree), so a test can assert on the presence/absence of a
/// particular control anywhere in the overlay's widget tree.
fn descendant_type_names(w: &dyn Widget, out: &mut Vec<&'static str>) {
    out.push(w.type_name());
    for c in w.children() {
        descendant_type_names(c.as_ref(), out);
    }
}

/// Open the toolbar's colour picker for the swatch at `row1_index` and return
/// the flattened `type_name`s of the whole overlay subtree. The picker cell is
/// flipped by clicking the swatch; a re-layout drives the overlay `Rebuilder`
/// to build the dialog before we introspect it.
fn open_picker_type_names(row1_index: usize) -> Vec<&'static str> {
    let (mut toolbar, handle) = toolbar_over(RichDoc::from_blocks(vec![Block::plain("hi")]));
    handle.select_all();
    toolbar.layout(Size::new(600.0, 100.0));

    {
        let col = &mut toolbar.children_mut()[0];
        let row1 = &mut col.children_mut()[0];
        let swatch = &mut row1.children_mut()[row1_index];
        let b = swatch.bounds();
        let pos = Point::new(b.width * 0.5, b.height * 0.5);
        swatch.on_event(&Event::MouseDown { pos, button: MouseButton::Left, modifiers: Modifiers::default() });
        swatch.on_event(&Event::MouseUp { pos, button: MouseButton::Left, modifiers: Modifiers::default() });
    }
    toolbar.layout(Size::new(600.0, 100.0));
    assert!(handle.is_previewing(), "opening the swatch begins a preview session");

    let mut names = Vec::new();
    descendant_type_names(toolbar.children()[1].as_ref(), &mut names);
    names
}

/// The Highlight picker must NOT expose the "No Color (Pass Through)" checkbox
/// in the rich-text toolbar: the user reported it as confusing here. (The core
/// `ColorWheelPicker` still supports it via `with_allow_none` for other hosts.)
///
/// The checkbox is the picker's only `Checkbox` child, so its absence anywhere
/// in the built overlay subtree proves `allow_none` is off.
#[test]
fn highlight_picker_has_no_no_color_checkbox() {
    // Row 1 without families: B/I/U/S(0-3), size combo(4), text-colour(5),
    // highlight(6).
    let names = open_picker_type_names(6);
    assert!(
        !names.contains(&"Checkbox"),
        "Highlight picker must not build a \"No Color (Pass Through)\" checkbox; \
         tree was {names:?}"
    );
}

/// The Text-colour picker must likewise omit the checkbox (it never applied a
/// `None` colour anyway, and the toggle is confusing).
#[test]
fn text_color_picker_has_no_no_color_checkbox() {
    let names = open_picker_type_names(5);
    assert!(
        !names.contains(&"Checkbox"),
        "Text-colour picker must not build a \"No Color (Pass Through)\" checkbox; \
         tree was {names:?}"
    );
}

// ── Remove-highlight button clears the selection's highlight ───────────────

/// Recursively test whether any descendant of `w` is a widget carrying a
/// `("text", glyph)` property — i.e. a `Label` (or a `Button`'s label child)
/// rendering `glyph`. Used to prove the eraser button exists in the tree.
fn descendant_renders_glyph(w: &dyn Widget, glyph: &str) -> bool {
    if w.properties().iter().any(|(k, v)| *k == "text" && v == glyph) {
        return true;
    }
    w.children().iter().any(|c| descendant_renders_glyph(c.as_ref(), glyph))
}

/// The Remove-highlight button is the sole UI route to `SetHighlight(None)` now
/// that the colour picker dropped its "No Color" checkbox. It must (a) exist in
/// the toolbar tree (its eraser glyph) and (b) clear a highlighted selection to
/// a uniform "no highlight" when clicked.
#[test]
fn remove_highlight_button_clears_selection_highlight() {
    let (mut toolbar, handle) = toolbar_over(RichDoc::from_blocks(vec![Block::plain("hi")]));
    handle.select_all();

    // Apply a highlight so the selection reports a concrete, uniform colour.
    let yellow = Color::from_rgb8(255, 240, 60);
    handle.exec(&RichCommand::SetHighlight(Some(yellow)));
    assert_eq!(
        handle.common_style_of_selection().highlight,
        Some(Some(yellow)),
        "selection is uniformly highlighted before the click"
    );

    toolbar.layout(Size::new(600.0, 100.0));

    // The eraser button must be present in the built toolbar tree.
    assert!(
        descendant_renders_glyph(toolbar.children()[0].as_ref(), ICON_ERASER),
        "toolbar row must contain the Remove-highlight (eraser) button"
    );

    // Row 1 (no families): B/I/U/S(0-3), size(4), text-colour(5), highlight(6),
    // remove-highlight(7).
    let button = {
        let col = &mut toolbar.children_mut()[0];
        let row1 = &mut col.children_mut()[0];
        &mut row1.children_mut()[7]
    };
    let b = button.bounds();
    let pos = Point::new(b.width * 0.5, b.height * 0.5);
    button.on_event(&Event::MouseDown { pos, button: MouseButton::Left, modifiers: Modifiers::default() });
    button.on_event(&Event::MouseUp { pos, button: MouseButton::Left, modifiers: Modifiers::default() });

    assert_eq!(
        handle.common_style_of_selection().highlight,
        Some(None),
        "clicking Remove highlight cleared the selection's highlight to None"
    );
}

// ── Drop guard: dropping mid-preview unwinds the session ───────────────────

/// Dropping the toolbar while its colour dialog is open mid-preview must cancel
/// the live-preview session, so the shared editor core isn't left with undo
/// suspended and a dangling snapshot.
#[test]
fn drop_mid_preview_cancels_session() {
    let (mut toolbar, handle) = toolbar_over(RichDoc::from_blocks(vec![Block::plain("hi")]));
    handle.select_all();
    toolbar.layout(Size::new(600.0, 100.0));

    // Click the text-colour swatch (row 1, index 5: B/I/U/S + size combo, then
    // text colour) to open the picker, then re-layout so the overlay's
    // Rebuilder builds the dialog and begins the preview session.
    {
        let col = &mut toolbar.children_mut()[0];
        let row1 = &mut col.children_mut()[0];
        let swatch = &mut row1.children_mut()[5];
        let b = swatch.bounds();
        let pos = Point::new(b.width * 0.5, b.height * 0.5);
        swatch.on_event(&Event::MouseDown { pos, button: MouseButton::Left, modifiers: Modifiers::default() });
        swatch.on_event(&Event::MouseUp { pos, button: MouseButton::Left, modifiers: Modifiers::default() });
    }
    toolbar.layout(Size::new(600.0, 100.0));
    assert!(handle.is_previewing(), "opening the swatch begins a preview session");

    drop(toolbar);
    assert!(
        !handle.is_previewing(),
        "dropping the toolbar mid-preview must cancel the session"
    );
}
