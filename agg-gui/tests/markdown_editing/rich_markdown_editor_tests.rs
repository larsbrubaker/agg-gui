//! agg-sharp's `RichMarkdownEditorTests.cs`, ported 1:1 against
//! `RichMarkdownEditor`: the toolbar driving the real editing widget through
//! the composite editor.  C#'s `Toolbar.XButton.InvokeClick()` is
//! `click(kind)` and `Editor.OnKeyDown` / `OnKeyPress` are `key_down` (agg-gui
//! delivers typed characters as `Key::Char` key downs).  `Editor.Focused`
//! after a toolbar click is read through an `App`, which services the focus
//! request at its next layout.

use std::sync::Arc;

use agg_gui::event::{Key, Modifiers};
use agg_gui::text::Font;
use agg_gui::widgets::markdown_editing::{
    DocPosition, MarkdownFormatButtonKind as K, RichMarkdownEditor, RichSelection,
};
use agg_gui::{App, Rect, Size, Widget};

use super::rich_markdown_edit_widget_tests::fonts;

const NONE: Modifiers = Modifiers {
    shift: false,
    ctrl: false,
    alt: false,
    meta: false,
};
const CTRL: Modifiers = Modifiers { ctrl: true, ..NONE };

fn icons() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::FONT_AWESOME_4_7).expect("font awesome"))
}

// Windows bindings, set per editor.
fn make(markdown: &str) -> RichMarkdownEditor {
    let mut editor = RichMarkdownEditor::new(fonts(), icons(), None);
    editor.set_markdown(markdown).unwrap();
    editor.editor_mut().set_use_mac_key_bindings(false);
    editor.layout(Size::new(500.0, 300.0));
    editor.set_bounds(Rect::new(0.0, 0.0, 500.0, 300.0));
    editor
}

fn editor(app: &mut App) -> &mut RichMarkdownEditor {
    app.root_mut()
        .as_any_mut()
        .and_then(|w| w.downcast_mut::<RichMarkdownEditor>())
        .expect("the root is the editor")
}

fn range(from: usize, to: usize) -> RichSelection {
    RichSelection::new(DocPosition::new(0, from), DocPosition::new(0, to))
}

#[test]
fn bold_from_the_toolbar_is_one_undo_step() {
    let mut editor = make("Hello world\n");
    editor.select(range(0, 5)).unwrap();

    editor.click(K::Bold).unwrap();
    assert_eq!(editor.markdown(), "**Hello** world\n");
    assert!(editor.button(K::Bold).is_pressed());

    editor.key_down(Key::Char('z'), CTRL);
    assert_eq!(editor.markdown(), "Hello world\n");
    assert!(!editor.button(K::Bold).is_pressed());
}

#[test]
fn a_click_that_changes_nothing_leaves_no_undo_step() {
    let mut editor = make("Hello\n");
    editor
        .select(RichSelection::at(DocPosition::new(0, 2)))
        .unwrap();

    editor.click(K::AlignLeft).unwrap();

    assert!(!editor.editor().state().history().can_undo());
}

#[test]
fn control_k_opens_the_link_row() {
    let mut editor = make("Hello world\n");
    editor.select(range(0, 5)).unwrap();

    editor.key_down(Key::Char('k'), CTRL);
    assert!(editor.toolbar().link_editor().is_visible());

    editor.toolbar().link_editor().set_url("example.com");
    editor.accept_link_editor().unwrap();
    assert_eq!(editor.markdown(), "[Hello](https://example.com) world\n");
}

#[test]
fn typing_after_bold_at_a_caret_types_bold() {
    let mut composite = RichMarkdownEditor::new(fonts(), icons(), None);
    composite.set_markdown("Hello world\n").unwrap();
    composite.editor_mut().set_use_mac_key_bindings(false);
    let mut app = App::new(Box::new(composite));
    app.layout(Size::new(500.0, 300.0));
    editor(&mut app)
        .select(RichSelection::at(DocPosition::new(0, 5)))
        .unwrap();

    editor(&mut app).click(K::Bold).unwrap();
    app.layout(Size::new(500.0, 300.0));
    assert!(editor(&mut app).editor().is_focused());
    assert_eq!(
        app.focused_widget_type_name(),
        Some("RichMarkdownEditWidget")
    );
    assert!(editor(&mut app).button(K::Bold).is_pressed());

    editor(&mut app).key_down(Key::Char('X'), NONE);
    assert_eq!(editor(&mut app).markdown(), "Hello**X** world\n");
}
