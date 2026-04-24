//! `font_picker` — reusable font-selection widget for the demo app.
//!
//! Returns a configured `ComboBox` set up for the bundled font table:
//! - One entry per font in `windows::system::FONT_OPTIONS`
//! - Each entry rendered in **its own typeface** (`with_item_fonts`)
//!   so the dropdown previews the look of each face at a glance, and
//!   the closed combo button shows the currently-selected font in
//!   that face
//! - Bound bidirectionally to the shared `font_index` cell on
//!   `windows::system::SystemCells` — picking a font in one window
//!   snaps every other picker in the app to the same selection on
//!   the next layout
//! - on_change automatically calls `apply_font_by_index`, which writes
//!   through to `font_settings::set_system_font`, the persisted
//!   `font_name` cell, and the shared `font_index` cell
//!
//! Drop `font_picker(font)` in anywhere a font choice is exposed —
//! the wiring (cell binding, font loading, per-item previews,
//! on-change side-effects) is all handled here so call sites stay
//! one line.

use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{ComboBox, Font, Widget};

use crate::windows::{
    apply_font_by_index, font_option_names, load_all_fonts, system_cells as cells,
};

/// Build a font-picker `ComboBox` ready to drop into any layout.
///
/// `label_font` is the typeface used for the closed combo's
/// selected-name label *only* if no per-item fonts are loaded — but
/// we always load them, so the closed combo previews in the selected
/// font.  Pass any reasonable fallback (the window's body font is
/// fine).
///
/// The returned box is the picker itself — no wrapping.  All
/// font-picker behaviour comes from `ComboBox`'s built-in features
/// plus the cell-binding + on-change wiring set up here.
pub fn font_picker(label_font: Arc<Font>) -> Box<dyn Widget> {
    let cells = cells();
    let names = font_option_names();
    let per_item = load_all_fonts();
    let initial_idx = cells.font_index.get().min(names.len().saturating_sub(1));

    let cells_for_change = cells.clone();
    Box::new(
        ComboBox::new(names, initial_idx, label_font)
            .with_font_size(13.0)
            .with_item_fonts(per_item)
            .with_selected_cell(Rc::clone(&cells.font_index))
            .on_change(move |idx| {
                apply_font_by_index(&cells_for_change, idx);
            }),
    )
}

/// Variant that lets the caller override the closed-combo's font size
/// (default is 13 pt to match the System window's body widgets).  Use
/// when the picker sits in a denser or sparser layout context.
pub fn font_picker_with_size(label_font: Arc<Font>, font_size: f64) -> Box<dyn Widget> {
    let cells = cells();
    let names = font_option_names();
    let per_item = load_all_fonts();
    let initial_idx = cells.font_index.get().min(names.len().saturating_sub(1));

    let cells_for_change = cells.clone();
    Box::new(
        ComboBox::new(names, initial_idx, label_font)
            .with_font_size(font_size)
            .with_item_fonts(per_item)
            .with_selected_cell(Rc::clone(&cells.font_index))
            .on_change(move |idx| {
                apply_font_by_index(&cells_for_change, idx);
            }),
    )
}
