//! `RichToolbar` — the two-row formatting toolbar for the RichTextEdit demo.
//!
//! Every control drives the shared [`RichEditHandle`] (bold/italic/underline/
//! strike toggles, font family + size, text/highlight colour, alignment, lists,
//! indent, undo/redo). Toggle buttons reflect live state through
//! [`Button::with_active_fn`], reading
//! [`RichEditHandle::common_style_of_selection`] each frame — cheap, since the
//! summary is recomputed only per selection change on the editor side.
//!
//! Colours open a floating [`color_wheel_picker_dialog`](agg_gui::color_wheel_picker_dialog)
//! managed by the parent window; the swatch buttons here just flip the shared
//! [`PickerKind`](super::PickerKind) open-state cell.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widgets::rich_text::{CommonStyle, ListKind};
use agg_gui::{
    Button, ComboBox, FlexRow, Font, RichCommand, RichEditHandle, TextHAlign, Widget,
};

use super::PickerKind;
use crate::windows::system_fonts::{font_option_index, font_option_names};

/// Font sizes offered by the size dropdown (points), matching common editors.
const FONT_SIZES: &[f64] = &[
    8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0,
];

// Font Awesome glyphs used as toolbar icons (rendered via the base font's FA
// fallback, per the project's icon convention).
const ICON_BOLD: &str = "\u{F032}";
const ICON_ITALIC: &str = "\u{F033}";
const ICON_UNDERLINE: &str = "\u{F0CD}";
const ICON_STRIKE: &str = "\u{F0CC}";
const ICON_ALIGN_LEFT: &str = "\u{F036}";
const ICON_ALIGN_CENTER: &str = "\u{F037}";
const ICON_ALIGN_RIGHT: &str = "\u{F038}";
const ICON_LIST_OL: &str = "\u{F0CB}";
const ICON_LIST_UL: &str = "\u{F0CA}";
const ICON_OUTDENT: &str = "\u{F03B}";
const ICON_INDENT: &str = "\u{F03C}";
const ICON_UNDO: &str = "\u{F0E2}";
const ICON_REDO: &str = "\u{F01E}";
const ICON_TEXT_COLOR: &str = "\u{F031}"; // "font"
const ICON_HIGHLIGHT: &str = "\u{F043}"; // "tint"

/// Build the two-row toolbar.
pub fn rich_toolbar(
    font: &Arc<Font>,
    handle: RichEditHandle,
    picker: Rc<Cell<PickerKind>>,
) -> Box<dyn Widget> {
    let mut col = agg_gui::FlexColumn::new().with_gap(6.0);
    col.push(row_one(font, &handle, &picker), 0.0);
    col.push(row_two(font, &handle), 0.0);
    Box::new(col)
}

/// Row 1: inline character formatting, font family + size, colours.
fn row_one(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    picker: &Rc<Cell<PickerKind>>,
) -> Box<dyn Widget> {
    let mut row = FlexRow::new().with_gap(4.0);

    row = row.add(style_toggle(font, handle, ICON_BOLD, |c| c.bold, RichCommand::ToggleBold));
    row = row.add(style_toggle(
        font,
        handle,
        ICON_ITALIC,
        |c| c.italic,
        RichCommand::ToggleItalic,
    ));
    row = row.add(style_toggle(
        font,
        handle,
        ICON_UNDERLINE,
        |c| c.underline,
        RichCommand::ToggleUnderline,
    ));
    row = row.add(style_toggle(
        font,
        handle,
        ICON_STRIKE,
        |c| c.strikethrough,
        RichCommand::ToggleStrikethrough,
    ));

    row = row.add(family_combo(font, handle));
    row = row.add(size_combo(font, handle));

    row = row.add(color_button(font, ICON_TEXT_COLOR, picker, PickerKind::TextColor));
    row = row.add(color_button(font, ICON_HIGHLIGHT, picker, PickerKind::Highlight));

    Box::new(row)
}

/// Row 2: alignment, lists, indent, undo/redo.
fn row_two(font: &Arc<Font>, handle: &RichEditHandle) -> Box<dyn Widget> {
    let mut row = FlexRow::new().with_gap(4.0);

    row = row.add(align_toggle(font, handle, ICON_ALIGN_LEFT, TextHAlign::Left));
    row = row.add(align_toggle(font, handle, ICON_ALIGN_CENTER, TextHAlign::Center));
    row = row.add(align_toggle(font, handle, ICON_ALIGN_RIGHT, TextHAlign::Right));

    row = row.add(command_button(
        font,
        handle,
        ICON_LIST_OL,
        RichCommand::SetList(ListKind::Ordered),
    ));
    row = row.add(command_button(
        font,
        handle,
        ICON_LIST_UL,
        RichCommand::SetList(ListKind::Bullet),
    ));
    row = row.add(command_button(font, handle, ICON_OUTDENT, RichCommand::Outdent));
    row = row.add(command_button(font, handle, ICON_INDENT, RichCommand::Indent));

    row = row.add(undo_button(font, handle));
    row = row.add(redo_button(font, handle));

    Box::new(row)
}

/// A bold/italic/underline/strike toggle whose active state reflects the
/// selection's [`CommonStyle`]. `Some(true)` = active; `Some(false)` / `None`
/// (mixed) render inactive — a deliberate simplification (a Button has no
/// tri-state), noted in the demo's module docs.
fn style_toggle(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    icon: &str,
    read: fn(&CommonStyle) -> Option<bool>,
    cmd: RichCommand,
) -> Box<dyn Widget> {
    let active_handle = handle.clone();
    let click_handle = handle.clone();
    Box::new(
        Button::new(icon, Arc::clone(font))
            .with_font_size(13.0)
            .with_subtle()
            .with_active_fn(move || read(&active_handle.common_style_of_selection()) == Some(true))
            .on_click(move || click_handle.exec(&cmd)),
    )
}

/// An alignment toggle, active when every selected block already has `align`.
fn align_toggle(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    icon: &str,
    align: TextHAlign,
) -> Box<dyn Widget> {
    let click_handle = handle.clone();
    Box::new(
        Button::new(icon, Arc::clone(font))
            .with_font_size(13.0)
            .with_subtle()
            .on_click(move || click_handle.exec(&RichCommand::SetAlign(align))),
    )
}

/// A plain command button (lists / indent) — fire-and-forget.
fn command_button(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    icon: &str,
    cmd: RichCommand,
) -> Box<dyn Widget> {
    let click_handle = handle.clone();
    Box::new(
        Button::new(icon, Arc::clone(font))
            .with_font_size(13.0)
            .with_subtle()
            .on_click(move || click_handle.exec(&cmd)),
    )
}

/// A colour swatch button that opens the floating picker for `kind`.
fn color_button(
    font: &Arc<Font>,
    icon: &str,
    picker: &Rc<Cell<PickerKind>>,
    kind: PickerKind,
) -> Box<dyn Widget> {
    let picker = Rc::clone(picker);
    Box::new(
        Button::new(icon, Arc::clone(font))
            .with_font_size(13.0)
            .with_subtle()
            .on_click(move || {
                picker.set(kind);
                agg_gui::animation::request_draw();
            }),
    )
}

/// Font-family dropdown (the whole system catalog). Each family renders in its
/// own face via the shared [`font_preview_combo`](crate::font_picker::font_preview_combo)
/// builder — preview faces load lazily and refresh as they arrive. Selecting a
/// family applies it via [`RichCommand::SetFontFamily`] (the resolver in the
/// parent window maps family + bold/italic to a concrete face) WITHOUT touching
/// the System window's font. The dropdown also reflects the selection's current
/// family each frame; a mixed selection leaves the last family shown.
fn family_combo(font: &Arc<Font>, handle: &RichEditHandle) -> Box<dyn Widget> {
    let default_idx = font_option_index("Arial").unwrap_or(0);
    let click_handle = handle.clone();
    let reflect_handle = handle.clone();
    let names_owned: Vec<String> = font_option_names().iter().map(|s| s.to_string()).collect();
    Box::new(
        crate::font_picker::font_preview_combo(Arc::clone(font), 12.0, default_idx, None, move |idx| {
            if let Some(name) = names_owned.get(idx) {
                // Load the face on demand so the resolver can pick it up.
                crate::windows::rich_text_demo::request_font(name);
                click_handle.exec(&RichCommand::SetFontFamily(name.clone()));
            }
        })
        .with_max_size(agg_gui::Size::new(150.0, 26.0))
        .with_reflect(move || family_reflection_index(&reflect_handle)),
    )
}

/// Map the selection's common family to a catalog index for the family
/// dropdown to highlight. `Some(Some(name))` is a consistent explicit family;
/// `Some(None)` is the default family (`Nunito`, the resolver's fallback);
/// `None` is a mixed selection, for which we return `None` so the dropdown
/// keeps its last highlight rather than flipping to a wrong one.
fn family_reflection_index(handle: &RichEditHandle) -> Option<usize> {
    match handle.common_style_of_selection().font_family {
        Some(Some(name)) => font_option_index(&name),
        Some(None) => font_option_index(crate::windows::DEFAULT_FONT_NAME),
        None => None,
    }
}

/// Font-size dropdown.
fn size_combo(font: &Arc<Font>, handle: &RichEditHandle) -> Box<dyn Widget> {
    let labels: Vec<String> = FONT_SIZES.iter().map(|s| format!("{}", *s as i64)).collect();
    // Default to 16 pt (the widget default).
    let default_idx = FONT_SIZES.iter().position(|s| *s == 16.0).unwrap_or(0);
    let click_handle = handle.clone();
    Box::new(
        ComboBox::new(labels, default_idx, Arc::clone(font))
            .with_font_size(12.0)
            .with_max_size(agg_gui::Size::new(64.0, 26.0))
            .on_change(move |idx| {
                if let Some(size) = FONT_SIZES.get(idx) {
                    click_handle.exec(&RichCommand::SetFontSize(*size));
                }
            }),
    )
}

fn undo_button(font: &Arc<Font>, handle: &RichEditHandle) -> Box<dyn Widget> {
    let enabled_handle = handle.clone();
    let click_handle = handle.clone();
    Box::new(
        Button::new(ICON_UNDO, Arc::clone(font))
            .with_font_size(13.0)
            .with_subtle()
            .with_enabled_fn(move || enabled_handle.can_undo())
            .on_click(move || click_handle.undo()),
    )
}

fn redo_button(font: &Arc<Font>, handle: &RichEditHandle) -> Box<dyn Widget> {
    let enabled_handle = handle.clone();
    let click_handle = handle.clone();
    Box::new(
        Button::new(ICON_REDO, Arc::clone(font))
            .with_font_size(13.0)
            .with_subtle()
            .with_enabled_fn(move || enabled_handle.can_redo())
            .on_click(move || click_handle.redo()),
    )
}
