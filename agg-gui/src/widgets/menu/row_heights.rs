//! Row heights, and the description line under a row's label
//! ([`MenuItem::description`]).
//!
//! A row is its label's line (the style's row height, a row's own
//! [`MenuItem::row_height`], floored at the touch minimum, or a widget row's
//! height) plus, for a described row, a band holding the description wrapped
//! to the row's width and [`DESCRIPTION_BOTTOM_PAD`] under it.  How tall the
//! wrapped words are needs the font, which the menu geometry does not have:
//! the layout asks a [`DescriptionHeight`] (the popup's
//! `super::fit_width::FitMeasure`, which wraps the words exactly as the row's
//! wrapping `Label` paints them) at the width each panel ends up with
//! (`geometry::stack_layout_placed`).  Without a font, a description counts as
//! one line of the metrics' text ([`description_band`]).
//!
//! [`RowLayout::line_rect`] is the label's line at the top of a row, where its
//! marks, icon, label, shortcut and chevron sit; [`RowLayout::description_rect`]
//! is the band's text area under it.  The row's whole rect is what hover, hit
//! testing and the keyboard highlight use.

use crate::geometry::Rect;

use super::geometry::{MenuMetrics, RowLayout};
use super::model::{MenuEntry, MenuItem};
use super::paint::MenuStyle;
use super::style::{row_label_right, row_label_x};

/// A description's text size, relative to its row's label.
pub const DESCRIPTION_SCALE: f64 = 0.85;
/// The room kept under a description, inside its band.
pub const DESCRIPTION_BOTTOM_PAD: f64 = 5.0;
/// The line height of a description, relative to its text size, when no font
/// measures it.
const DESCRIPTION_LINE: f64 = 1.3;

/// The height of `item`'s description wrapped within a row `row_width` wide,
/// or `None` when it can't be measured (no font): the layout then counts one
/// line ([`description_band`]).
pub type DescriptionHeight<'a> = dyn Fn(&MenuItem, f64) -> Option<f64> + 'a;

/// A [`DescriptionHeight`] with no font: every description is one line.
pub fn one_line_descriptions(_item: &MenuItem, _row_width: f64) -> Option<f64> {
    None
}

impl RowLayout {
    /// The label's line: the whole row for a one-line row, the part above
    /// the description band for a described one.  Marks, icon, label,
    /// shortcut and chevron are centred on it.
    pub fn line_rect(&self) -> Rect {
        Rect::new(
            self.rect.x,
            self.rect.y + self.description_height,
            self.rect.width,
            (self.rect.height - self.description_height).max(0.0),
        )
    }

    /// The description's text area inside its band (Y-up: the band's bottom
    /// [`DESCRIPTION_BOTTOM_PAD`] stays clear); empty for a one-line row.
    pub fn description_rect(&self) -> Rect {
        let pad = DESCRIPTION_BOTTOM_PAD.min(self.description_height);
        Rect::new(
            self.rect.x,
            self.rect.y + pad,
            self.rect.width,
            self.description_height - pad,
        )
    }

    /// The area `item`'s description wraps in under `style`: the
    /// [`Self::description_rect`] from the label's edge to the row's
    /// right-hand column, the room `FitMeasure::description_height` measures
    /// and the row paints into.
    pub fn description_text_rect(&self, style: &MenuStyle, item: &MenuItem) -> Rect {
        let band = self.description_rect();
        let left = row_label_x(style.label_x, style.selection_column, item);
        let right = row_label_right(style.shortcut_right, item);
        Rect::new(
            band.x + left,
            band.y,
            (band.width - left - right).max(0.0),
            band.height,
        )
    }
}

/// How much a one-line [`MenuItem::description`] adds to its row when no
/// font measures it: a line of the metrics' text size scaled by
/// [`DESCRIPTION_SCALE`] (so it grows with a finger-sized menu) and
/// [`DESCRIPTION_BOTTOM_PAD`] under it, rounded up to a whole logical px.
pub fn description_band(m: &MenuMetrics) -> f64 {
    (m.default_font_size * DESCRIPTION_SCALE * DESCRIPTION_LINE + DESCRIPTION_BOTTOM_PAD).ceil()
}

/// The band a description `text_height` tall needs: the text and
/// [`DESCRIPTION_BOTTOM_PAD`], rounded up to a whole logical px.
fn band_for(text_height: f64) -> f64 {
    (text_height + DESCRIPTION_BOTTOM_PAD).ceil()
}

/// Whether `entry` is a row that shows a description.
fn described(entry: &MenuEntry) -> Option<&MenuItem> {
    match entry {
        MenuEntry::Item(item) if item.widget_row.is_none() && item.description.is_some() => {
            Some(item)
        }
        _ => None,
    }
}

/// The description band of `entry`'s row with no font: [`description_band`]
/// for an item row with a description, `0.0` for any other.
pub fn entry_description_height(entry: &MenuEntry, m: &MenuMetrics) -> f64 {
    if described(entry).is_some() {
        description_band(m)
    } else {
        0.0
    }
}

/// The height of `entry`'s label line: separators are thin, widget rows
/// take their own height, an item with a [`MenuItem::row_height`] takes that
/// (floored at the metrics' [`MenuMetrics::touch_row_min`], as the style's
/// row is), every other item takes the (possibly touch-grown) row height.
fn line_height(entry: &MenuEntry, m: &MenuMetrics) -> f64 {
    match entry {
        MenuEntry::Item(item) => match (item.widget_row, item.row_height) {
            (Some(w), _) => w.height,
            (None, Some(h)) if h.is_finite() && h > 0.0 => h.max(m.touch_row_min),
            _ => m.row_h,
        },
        MenuEntry::Separator => m.sep_h,
    }
}

/// Height of one entry's row with no font: its label line
/// ([`line_height`]'s rules) and, for a described item, a one-line
/// [`description_band`] under it.
pub fn entry_height(entry: &MenuEntry, m: &MenuMetrics) -> f64 {
    line_height(entry, m) + entry_description_height(entry, m)
}

/// One row's height and the description band at its bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RowHeight {
    pub total: f64,
    pub description: f64,
}

/// Every row of a panel `row_width` wide: each description wrapped to that
/// width by `descriptions` (one line when it can't measure).
pub(crate) fn level_row_heights(
    items: &[MenuEntry],
    m: &MenuMetrics,
    row_width: f64,
    descriptions: &DescriptionHeight<'_>,
) -> Vec<RowHeight> {
    items
        .iter()
        .map(|entry| {
            let description = described(entry).map_or(0.0, |item| {
                descriptions(item, row_width).map_or_else(|| description_band(m), band_for)
            });
            RowHeight {
                total: line_height(entry, m) + description,
                description,
            }
        })
        .collect()
}

/// A panel's content height: its rows, never less than one row.
pub(crate) fn content_height(heights: &[RowHeight], m: &MenuMetrics) -> f64 {
    heights.iter().map(|h| h.total).sum::<f64>().max(m.row_h)
}
