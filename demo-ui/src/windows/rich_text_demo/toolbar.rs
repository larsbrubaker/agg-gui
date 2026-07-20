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

use agg_gui::platform::primary_modifier_label;
use agg_gui::widgets::rich_text::{CommonStyle, ListKind};
use agg_gui::{
    Button, ComboBox, FlexRow, Font, RichCommand, RichEditHandle, TextHAlign, Widget,
};

use super::PickerKind;
use crate::windows::system_fonts::{
    family_has_bold, family_has_italic, font_option_index, font_option_names,
};

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
const ICON_REMOVE_HIGHLIGHT: &str = "\u{F12D}"; // "eraser"

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

/// Attach hover-help `text` to `w` via the first-class tooltip system
/// (`Widget::set_tooltip_text` → `WidgetBase::tooltip`); the central controller
/// shows it on hover, no wrapper widget. Mirrors the library toolbar's
/// default-on tooltips ([`RichTextToolbar::with_tooltips`](agg_gui::widgets::rich_text::toolbar)).
/// `_font` is retained so call sites read uniformly with the library toolbar.
fn tip(mut w: Box<dyn Widget>, text: impl Into<String>, _font: &Arc<Font>) -> Box<dyn Widget> {
    w.set_tooltip_text(Some(text.into()));
    w
}

/// Row 1: inline character formatting, font family + size, colours.
fn row_one(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    picker: &Rc<Cell<PickerKind>>,
) -> Box<dyn Widget> {
    let mut row = FlexRow::new().with_gap(4.0);

    // Bold / Italic gate on the current family actually shipping that variant
    // (`family_has_bold` / `family_has_italic`); Underline / Strikethrough are
    // synthetic and stay always-enabled.
    row = row.add(tip(
        style_toggle(font, handle, ICON_BOLD, |c| c.bold, RichCommand::ToggleBold, Some(family_has_bold)),
        "Bold",
        font,
    ));
    row = row.add(tip(
        style_toggle(font, handle, ICON_ITALIC, |c| c.italic, RichCommand::ToggleItalic, Some(family_has_italic)),
        "Italic",
        font,
    ));
    row = row.add(tip(
        style_toggle(font, handle, ICON_UNDERLINE, |c| c.underline, RichCommand::ToggleUnderline, None),
        "Underline",
        font,
    ));
    row = row.add(tip(
        style_toggle(font, handle, ICON_STRIKE, |c| c.strikethrough, RichCommand::ToggleStrikethrough, None),
        "Strikethrough",
        font,
    ));

    row = row.add(tip(family_combo(font, handle), "Font family", font));
    row = row.add(tip(size_combo(font, handle), "Font size", font));

    row = row.add(tip(color_button(font, ICON_TEXT_COLOR, picker, PickerKind::TextColor), "Text color", font));
    row = row.add(tip(color_button(font, ICON_HIGHLIGHT, picker, PickerKind::Highlight), "Highlight color", font));
    row = row.add(tip(remove_highlight_button(font, handle), "Remove highlight", font));

    Box::new(row)
}

/// Row 2: alignment, lists, indent, undo/redo.
fn row_two(font: &Arc<Font>, handle: &RichEditHandle) -> Box<dyn Widget> {
    let modifier = primary_modifier_label();
    let mut row = FlexRow::new().with_gap(4.0);

    row = row.add(tip(align_toggle(font, handle, ICON_ALIGN_LEFT, TextHAlign::Left), "Align left", font));
    row = row.add(tip(align_toggle(font, handle, ICON_ALIGN_CENTER, TextHAlign::Center), "Align center", font));
    row = row.add(tip(align_toggle(font, handle, ICON_ALIGN_RIGHT, TextHAlign::Right), "Align right", font));

    row = row.add(tip(list_toggle(font, handle, ICON_LIST_OL, ListKind::Ordered), "Numbered list", font));
    row = row.add(tip(list_toggle(font, handle, ICON_LIST_UL, ListKind::Bullet), "Bulleted list", font));
    row = row.add(tip(command_button(font, handle, ICON_OUTDENT, RichCommand::Outdent), "Decrease indent", font));
    row = row.add(tip(command_button(font, handle, ICON_INDENT, RichCommand::Indent), "Increase indent", font));

    // Undo/redo are the only actions with a real key binding (the editor binds
    // `{mod}+Z` / `{mod}+Y`), so they get a shortcut hint.
    row = row.add(tip(undo_button(font, handle), format!("Undo ({modifier}+Z)"), font));
    row = row.add(tip(redo_button(font, handle), format!("Redo ({modifier}+Y)"), font));

    Box::new(row)
}

/// A bold/italic/underline/strike toggle whose active state reflects the
/// selection's [`CommonStyle`]. `Some(true)` = active; `Some(false)` / `None`
/// (mixed) render inactive — a deliberate simplification (a Button has no
/// tri-state), noted in the demo's module docs.
///
/// `family_supports`, when `Some`, gates the button on the selection's
/// effective family actually shipping that variant (see
/// [`family_variant_enabled`]): Bold/Italic disable for families with no such
/// face; Underline/Strikethrough pass `None` and stay always-enabled.
fn style_toggle(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    icon: &str,
    read: fn(&CommonStyle) -> Option<bool>,
    cmd: RichCommand,
    family_supports: Option<fn(&str) -> bool>,
) -> Box<dyn Widget> {
    let active_handle = handle.clone();
    let click_handle = handle.clone();
    let button = Button::new(icon, Arc::clone(font))
        .with_font_size(13.0)
        .with_subtle()
        .with_active_fn(move || read(&active_handle.common_style_of_selection()) == Some(true))
        .on_click(move || click_handle.exec(&cmd));
    let button = if let Some(check) = family_supports {
        let enabled_handle = handle.clone();
        button.with_enabled_fn(move || {
            family_variant_enabled(
                &enabled_handle.common_style_of_selection().font_family,
                check,
            )
        })
    } else {
        button
    };
    Box::new(button)
}

/// Decide whether a family-gated toggle (Bold / Italic) should be enabled for a
/// selection's common `font_family`:
/// - `Some(Some(name))` — a consistent explicit family: gate on `check(name)`.
/// - `Some(None)` — the inherited default family: gate on the default's variant.
/// - `None` — a mixed selection: keep enabled (the user may be narrowing it).
fn family_variant_enabled(family: &Option<Option<String>>, check: fn(&str) -> bool) -> bool {
    match family {
        Some(Some(name)) => check(name),
        Some(None) => check(crate::windows::DEFAULT_FONT_NAME),
        None => true,
    }
}

/// An alignment toggle. Behaves like a radio group: active precisely when
/// every selected block already carries `align` (`common_style.align ==
/// Some(align)`), so exactly one of L/C/R lights up for a consistent
/// selection and none do for a mixed one. Same subtle→accent look as B/I/U/S.
fn align_toggle(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    icon: &str,
    align: TextHAlign,
) -> Box<dyn Widget> {
    let active_handle = handle.clone();
    let click_handle = handle.clone();
    Box::new(
        Button::new(icon, Arc::clone(font))
            .with_font_size(13.0)
            .with_subtle()
            .with_active_fn(move || active_handle.common_style_of_selection().align == Some(align))
            .on_click(move || click_handle.exec(&RichCommand::SetAlign(align))),
    )
}

/// An ordered/bullet list toggle, active only when every selected block is
/// already that list `kind` (`common_style.list == Some(kind)`). A mixed
/// selection reports `None` and an all-plain one reports `Some(ListKind::None)`,
/// so in both cases neither list button reads as active.
fn list_toggle(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    icon: &str,
    kind: ListKind,
) -> Box<dyn Widget> {
    let active_handle = handle.clone();
    let click_handle = handle.clone();
    Box::new(
        Button::new(icon, Arc::clone(font))
            .with_font_size(13.0)
            .with_subtle()
            .with_active_fn(move || active_handle.common_style_of_selection().list == Some(kind))
            .on_click(move || click_handle.exec(&RichCommand::SetList(kind))),
    )
}

/// A momentary action button (indent / outdent) — fire-and-forget. Rendered as
/// a plain ghost that never enters the active/accent state: `with_active_fn(||
/// false)` forces the muted ghost look so it can't be mistaken for a toggle.
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
            .with_ghost()
            .with_active_fn(|| false)
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

/// A momentary button that clears the selection's highlight via
/// `SetHighlight(None)` — the sole UI route to un-highlight text now that the
/// colour picker dropped its "No Color" checkbox. Mirrors the library toolbar's
/// [`remove_highlight_button`](agg_gui::widgets::rich_text::toolbar).
fn remove_highlight_button(font: &Arc<Font>, handle: &RichEditHandle) -> Box<dyn Widget> {
    let click_handle = handle.clone();
    Box::new(
        Button::new(ICON_REMOVE_HIGHLIGHT, Arc::clone(font))
            .with_font_size(13.0)
            .with_subtle()
            .with_active_fn(|| false)
            .on_click(move || click_handle.exec(&RichCommand::SetHighlight(None))),
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
        // Keep the family dropdown compact: the per-family preview label can be
        // wide, so cap it at a fixed width instead of letting the flex row
        // stretch it across the whole toolbar. `FontPreviewCombo` forwards this
        // `max_size` to its inner `ComboBox`, which the `FlexRow` reads to clamp
        // the slot — see `font_picker::FontPreviewCombo::max_size`.
        .with_max_size(agg_gui::Size::new(180.0, 26.0))
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

/// Undo / redo are momentary actions gated on availability: a ghost button
/// (never active) that greys out via `enabled_fn` when there is nothing to
/// undo/redo — consistent with the indent/outdent buttons beside them.
fn undo_button(font: &Arc<Font>, handle: &RichEditHandle) -> Box<dyn Widget> {
    let enabled_handle = handle.clone();
    let click_handle = handle.clone();
    Box::new(
        Button::new(ICON_UNDO, Arc::clone(font))
            .with_font_size(13.0)
            .with_ghost()
            .with_active_fn(|| false)
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
            .with_ghost()
            .with_active_fn(|| false)
            .with_enabled_fn(move || enabled_handle.can_redo())
            .on_click(move || click_handle.redo()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    use agg_gui::widgets::rich_text::{InlineStyle, RichDoc};
    use agg_gui::{RichTextEdit, SharedResolver};

    fn test_font() -> Arc<Font> {
        const BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");
        Arc::new(Font::from_bytes(BYTES.to_vec()).expect("load test font"))
    }

    /// Build the toolbar over a throwaway editor handle. Structural only — no
    /// layout or paint runs, so the trivial resolver below is never invoked.
    fn build_toolbar(font: &Arc<Font>) -> Box<dyn Widget> {
        let resolver: SharedResolver = {
            let f = Arc::clone(font);
            Rc::new(move |_: &InlineStyle| Arc::clone(&f))
        };
        let editor = RichTextEdit::new(RichDoc::default(), resolver);
        let handle = editor.handle();
        let picker = Rc::new(Cell::new(PickerKind::None));
        rich_toolbar(font, handle, picker)
    }

    /// A control's own `type_name`, peering through the hover-`Tooltip` wrapper
    /// every control now carries so the roster assertion reads the underlying
    /// control, not the wrapper.
    fn unwrap_tip(w: &dyn Widget) -> &dyn Widget {
        if w.type_name() == "Tooltip" {
            w.children()[0].as_ref()
        } else {
            w
        }
    }

    fn child_type_names(w: &dyn Widget) -> Vec<&'static str> {
        w.children()
            .iter()
            .map(|c| unwrap_tip(c.as_ref()).type_name())
            .collect()
    }

    /// Regression guard for the font-preview refactor (commit 878be6d): the
    /// family dropdown switched to a `FontPreviewCombo`, and the size ComboBox
    /// plus the text-colour and highlight buttons must NOT be dropped. A tree
    /// walk by `type_name` pins the exact control roster of both rows so a
    /// future refactor cannot silently lose one.
    #[test]
    fn toolbar_keeps_full_control_roster() {
        let toolbar = build_toolbar(&test_font());
        assert_eq!(toolbar.children().len(), 2, "toolbar has two rows");

        // Row 1: B / I / U / S, family preview combo (FontPicker), size combo
        // (ComboBox), then the text-colour and highlight swatch buttons.
        assert_eq!(
            child_type_names(toolbar.children()[0].as_ref()),
            [
                "Button", "Button", "Button", "Button", "FontPicker", "ComboBox", "Button",
                "Button", "Button",
            ],
            "row 1 lost a control (size combo, a colour button, or remove-highlight)"
        );

        // Row 2: 3 alignments + ordered/bullet lists + outdent/indent + undo/redo.
        assert_eq!(
            child_type_names(toolbar.children()[1].as_ref()),
            vec!["Button"; 9],
            "row 2 control roster changed"
        );
    }

    /// Bold/Italic gating: a family with the variant keeps the toggle enabled,
    /// a family without it disables the toggle, the inherited default (`None`)
    /// follows the default family (Nunito → both available), and a mixed
    /// selection (`None` outer) stays enabled.
    #[test]
    fn family_variant_gating() {
        // Explicit family with a real Bold + Italic (Nunito).
        let nunito = Some(Some("Nunito".to_string()));
        assert!(family_variant_enabled(&nunito, family_has_bold));
        assert!(family_variant_enabled(&nunito, family_has_italic));

        // Explicit family with italic but no bold (Arial): Bold off, Italic on.
        let arial = Some(Some("Arial".to_string()));
        assert!(!family_variant_enabled(&arial, family_has_bold));
        assert!(family_variant_enabled(&arial, family_has_italic));

        // Inherited default family -> follows the default (Nunito): both on.
        let inherited: Option<Option<String>> = Some(None);
        assert!(family_variant_enabled(&inherited, family_has_bold));
        assert!(family_variant_enabled(&inherited, family_has_italic));

        // Mixed selection -> keep enabled regardless of the variant check.
        let mixed: Option<Option<String>> = None;
        assert!(family_variant_enabled(&mixed, family_has_bold));
        assert!(family_variant_enabled(&mixed, family_has_italic));
    }
}
