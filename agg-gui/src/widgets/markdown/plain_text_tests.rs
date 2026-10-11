//! Tests for [`MarkdownView`]'s plain text, public selection and read-only
//! introspection (`introspect.rs`): the plain text keeps the spaces between
//! words, also where a row wraps and between list items (a fix: the
//! selection model used to join wrapped rows and consecutive paragraphs with
//! nothing, so select-all copied "Thiseditorshows"); `set_selection` selects
//! a range of it for Ctrl+C; blocks, table cells and image widths answer what
//! C#'s tests read off MarkdigAgg's child widgets. Ctrl+C copies the
//! selected words as text, as C# does.

use std::sync::Arc;

use crate::event::{Event, Key, Modifiers};
use crate::geometry::Size;
use crate::text::Font;
use crate::widget::Widget;

use super::{MarkdownBlockKind, MarkdownView};

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

const REFERENCE_START: &str = "# Markdown Reference\n\nThis editor shows the same Markdown features that MatterCAD help articles render today.\n\n## Supported Features\n\n- Headings\n- Paragraphs\n- Bold text\n";

fn view_at(markdown: &str, width: f64) -> MarkdownView {
    crate::widget::set_current_viewport(Size::new(width, 320.0));
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
    let mut view = MarkdownView::new(markdown, font);
    view.layout(Size::new(width, 2000.0));
    view
}

#[test]
fn rust_only_plain_text_keeps_spaces_between_words() {
    let v = view_at(REFERENCE_START, 900.0);
    assert_eq!(
        v.plain_text(),
        "Markdown Reference\nThis editor shows the same Markdown features that MatterCAD help articles render today.\nSupported Features\n• Headings\n• Paragraphs\n• Bold text\n"
    );
}

#[test]
fn rust_only_plain_text_joins_wrapped_rows_with_a_space() {
    let v = view_at(REFERENCE_START, 140.0);
    let text = v.plain_text();
    assert!(
        text.contains("This editor shows the same Markdown features that MatterCAD help articles render today."),
        "{text:?}"
    );
    assert!(text.contains("• Headings\n• Paragraphs"), "{text:?}");
}

#[test]
fn rust_only_select_all_copies_the_words_with_their_spaces() {
    let _clipboard = crate::clipboard::simulate();
    let mut v = view_at(REFERENCE_START, 140.0);
    v.on_event(&Event::FocusGained);
    let cmd = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    v.on_event(&Event::KeyDown {
        key: Key::Char('a'),
        modifiers: cmd,
    });
    v.on_event(&Event::KeyDown {
        key: Key::Char('c'),
        modifiers: cmd,
    });
    let copied = crate::clipboard::get_text().unwrap_or_default();
    assert!(copied.contains("This editor shows the same"), "{copied:?}");
}

/// The selection half of C# `MarkdownTablePreviewTests.HelpTabCtrlCCopiesSelectedTextAsHtml`.
#[test]
fn set_selection_selects_plain_text_and_ctrl_c_copies_it() {
    let _clipboard = crate::clipboard::simulate();
    let mut v = view_at(REFERENCE_START, 900.0);
    let start = v.plain_text().find("Markdown").unwrap();
    v.set_selection(start, start + "Markdown".len());
    assert_eq!(v.selected_text(), "Markdown");
    assert_eq!(v.selection(), Some((start, start + 8)));
    v.on_event(&Event::FocusGained);
    v.on_event(&Event::KeyDown {
        key: Key::Char('c'),
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    });
    assert_eq!(crate::clipboard::get_text().as_deref(), Some("Markdown"));
}

#[test]
fn a_selection_set_before_layout_is_clamped_at_layout() {
    crate::widget::set_current_viewport(Size::new(400.0, 320.0));
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
    let mut v = MarkdownView::new("Hello", font);
    v.set_selection(1, 99);
    v.layout(Size::new(400.0, 2000.0));
    assert_eq!(
        v.selected_text(),
        "ello\n",
        "the end clamps to the text's end"
    );
}

#[test]
fn blocks_list_quote_code_and_table_in_order() {
    let v = view_at(
        "> a quote\n> more\n\n```\ncode\n```\n\n| a | b |\n|---|---|\n| c | d |\n\n> second\n",
        600.0,
    );
    assert_eq!(
        v.blocks(),
        vec![
            MarkdownBlockKind::Quote,
            MarkdownBlockKind::Code,
            MarkdownBlockKind::Table,
            MarkdownBlockKind::Quote,
        ]
    );
}

#[test]
fn table_cells_report_their_column_widths_and_visible_widths() {
    let v = view_at(
        "| Name | Value |\n|---|---|\n| x | a longer value |\n",
        600.0,
    );
    let tables = v.table_cells();
    assert_eq!(tables.len(), 1);
    let cells = &tables[0];
    assert_eq!(cells.len(), 4);
    assert_eq!(cells[3].text, "a longer value");
    assert_eq!((cells[3].row, cells[3].column), (1, 1));
    assert_eq!(cells[0].width, cells[2].width, "a column shares one width");
    assert!(cells
        .iter()
        .all(|c| c.width > 40.0 && c.visible_width == c.width));
}

#[test]
fn a_cell_past_a_narrow_viewport_shows_only_its_visible_part() {
    let v = view_at(
        "| aaaaaaaaaaaaaaaaaaaa | bbbbbbbbbbbbbbbbbbbb |\n|---|---|\n| c | d |\n",
        200.0,
    );
    let cells = &v.table_cells()[0];
    assert!(cells[1].visible_width < cells[1].width);
}

#[test]
fn image_widths_list_each_laid_out_image() {
    let v = view_at("![one](a.png) text ![two](b.png)", 600.0);
    let widths = v.image_widths();
    assert_eq!(widths.len(), 2);
    assert!(widths.iter().all(|w| *w > 0.0));
}
