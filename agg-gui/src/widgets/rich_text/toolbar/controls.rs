//! Individual toolbar controls for [`RichTextToolbar`](super::RichTextToolbar).
//!
//! Each function builds one ghost/subtle [`Button`] (or a [`ComboBox`]) wired to
//! a shared [`RichEditHandle`]: toggles reflect live state through
//! [`Button::with_active_fn`] reading
//! [`RichEditHandle::common_style_of_selection`], momentary actions fire a
//! [`RichCommand`] on click, and undo/redo grey out via `enabled_fn`.  This
//! mirrors the demo toolbar (`demo-ui/.../rich_text_demo/toolbar.rs`) but drives
//! only library types — no dependency on the demo's system-font catalog.

use std::rc::Rc;
use std::sync::Arc;

use crate::geometry::Size;
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::button::Button;
use crate::widgets::combo_box::ComboBox;
use crate::widgets::flex_row::FlexRow;
use crate::widgets::text_area::TextHAlign;

use super::super::commands::{CommonStyle, RichCommand};
use super::super::editor::RichEditHandle;
use super::super::model::ListKind;
use super::{Families, Variant, VariantCheck};

// Font Awesome glyphs used as toolbar icons (rendered via the base font's FA
// fallback, per the project's icon convention).
pub(super) const ICON_BOLD: &str = "\u{F032}";
pub(super) const ICON_ITALIC: &str = "\u{F033}";
pub(super) const ICON_UNDERLINE: &str = "\u{F0CD}";
pub(super) const ICON_STRIKE: &str = "\u{F0CC}";
pub(super) const ICON_ALIGN_LEFT: &str = "\u{F036}";
pub(super) const ICON_ALIGN_CENTER: &str = "\u{F037}";
pub(super) const ICON_ALIGN_RIGHT: &str = "\u{F038}";
pub(super) const ICON_LIST_OL: &str = "\u{F0CB}";
pub(super) const ICON_LIST_UL: &str = "\u{F0CA}";
pub(super) const ICON_OUTDENT: &str = "\u{F03B}";
pub(super) const ICON_INDENT: &str = "\u{F03C}";
pub(super) const ICON_UNDO: &str = "\u{F0E2}";
pub(super) const ICON_REDO: &str = "\u{F01E}";

/// A new empty formatting row with the toolbar's standard gap.
pub(super) fn new_row() -> FlexRow {
    FlexRow::new().with_gap(4.0)
}

/// A bold/italic/underline/strike toggle whose active state reflects the
/// selection's [`CommonStyle`]. `Some(true)` = active; `Some(false)` / `None`
/// (mixed) render inactive — a `Button` has no tri-state.
///
/// When `variant` + `check` are both supplied, Bold/Italic gate on the
/// selection's effective family actually shipping that variant (see
/// [`variant_enabled`]); Underline/Strikethrough pass `None` and stay enabled.
pub(super) fn style_toggle(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    icon: &str,
    read: fn(&CommonStyle) -> Option<bool>,
    cmd: RichCommand,
    variant: Option<Variant>,
    check: Option<&VariantCheck>,
) -> Box<dyn Widget> {
    let active_handle = handle.clone();
    let click_handle = handle.clone();
    let button = Button::new(icon, Arc::clone(font))
        .with_font_size(13.0)
        .with_subtle()
        .with_active_fn(move || read(&active_handle.common_style_of_selection()) == Some(true))
        .on_click(move || click_handle.exec(&cmd));
    let button = match (variant, check) {
        (Some(v), Some(check)) => {
            let enabled_handle = handle.clone();
            let check = Rc::clone(check);
            button.with_enabled_fn(move || {
                variant_enabled(
                    &enabled_handle.common_style_of_selection().font_family,
                    &check,
                    v,
                )
            })
        }
        _ => button,
    };
    Box::new(button)
}

/// Decide whether a family-gated toggle (Bold / Italic) is enabled for a
/// selection's common `font_family`:
/// - `Some(Some(name))` — a consistent explicit family: gate on `check(name, v)`.
/// - `Some(None)` — the inherited default family (name unknown to the library):
///   keep enabled; the resolver picks the face.
/// - `None` — a mixed selection: keep enabled.
pub(super) fn variant_enabled(
    family: &Option<Option<String>>,
    check: &VariantCheck,
    v: Variant,
) -> bool {
    match family {
        Some(Some(name)) => check(name, v),
        Some(None) | None => true,
    }
}

/// An alignment toggle. Behaves like a radio group: active precisely when every
/// selected block already carries `align`, so exactly one of L/C/R lights up for
/// a consistent selection and none do for a mixed one.
pub(super) fn align_toggle(
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
/// already that list `kind`.
pub(super) fn list_toggle(
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

/// A momentary action button (indent / outdent) — a plain ghost that never
/// enters the active/accent state so it can't be mistaken for a toggle.
pub(super) fn command_button(
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

/// Undo — a ghost button that greys out when there is nothing to undo.
pub(super) fn undo_button(font: &Arc<Font>, handle: &RichEditHandle) -> Box<dyn Widget> {
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

/// Redo — the mirror of [`undo_button`].
pub(super) fn redo_button(font: &Arc<Font>, handle: &RichEditHandle) -> Box<dyn Widget> {
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

/// Font-family dropdown over the injected `families`. Selecting a family applies
/// it via [`RichCommand::SetFontFamily`]; the app's resolver maps family +
/// bold/italic to a concrete face. Optional `item_fonts` render each row in its
/// own face. Unlike the demo's `FontPreviewCombo`, this simple `ComboBox` does
/// not reflect the selection's current family back into the closed label.
pub(super) fn family_combo(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    families: &Families,
) -> Box<dyn Widget> {
    let names = families.names.clone();
    let click_handle = handle.clone();
    let mut combo = ComboBox::new(families.names.clone(), 0, Arc::clone(font))
        .with_font_size(12.0)
        .with_max_size(Size::new(180.0, 26.0))
        .on_change(move |idx| {
            if let Some(name) = names.get(idx) {
                click_handle.exec(&RichCommand::SetFontFamily(name.clone()));
            }
        });
    if let Some(fonts) = &families.item_fonts {
        combo = combo.with_item_fonts(fonts.clone());
    }
    Box::new(combo)
}

/// Font-size dropdown over `sizes` (points). Selecting a size applies it via
/// [`RichCommand::SetFontSize`]. Defaults the closed label to 16 pt (the editor
/// default) when present, else the first entry.
pub(super) fn size_combo(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    sizes: &[f64],
) -> Box<dyn Widget> {
    let labels: Vec<String> = sizes.iter().map(|s| format!("{}", *s as i64)).collect();
    let default_idx = sizes.iter().position(|s| *s == 16.0).unwrap_or(0);
    let sizes = sizes.to_vec();
    let click_handle = handle.clone();
    Box::new(
        ComboBox::new(labels, default_idx, Arc::clone(font))
            .with_font_size(12.0)
            .with_max_size(Size::new(64.0, 26.0))
            .on_change(move |idx| {
                if let Some(size) = sizes.get(idx) {
                    click_handle.exec(&RichCommand::SetFontSize(*size));
                }
            }),
    )
}
