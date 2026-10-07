//! Popup width policy: the fixed default width or a content-fitted width.
//!
//! [`MenuWidth::Fixed`] (the default) keeps every popup at
//! [`super::geometry::MENU_W`] (touch-grown by [`super::effective_metrics`]).
//! [`MenuWidth::FitContent`] sizes each popup level — root and every cascaded
//! submenu independently — to its widest row's natural width, the way
//! agg-sharp's `PopupMenu` sizes itself (`HAnchor.Fit` over rows that are
//! `MaxFitOrStretch`).
//!
//! The rule, translated from agg-sharp `PopupMenu.MenuItem` onto this crate's
//! row layout ([`super::paint::MenuStyle`]):
//!
//! * the label starts at `style.label_x` (the icon gutter plus row inset —
//!   agg-sharp's `MenuGutterWidth` left padding plus `MenuRowInset`);
//! * a shortcut sits [`SHORTCUT_GAP`] past the label (agg-sharp's
//!   `MenuPadding` right of 20 on the label, which is the separation it
//!   intends) and ends `style.shortcut_right` from the row's right edge;
//! * a row without a shortcut still keeps `style.shortcut_right` on the
//!   right, which is where the submenu chevron and right-edge check marks
//!   paint;
//! * no row is narrower than [`FIT_MIN_W`] — agg-sharp's 150 px
//!   `MenuItem.MinimumSize` plus its 3 px `MenuRowInset` on both sides — grown
//!   in lock-step with the touch metrics like the fixed width is.  A host can
//!   lower or drop that floor per popup ([`super::PopupMenu::with_min_width`],
//!   stored in [`super::PopupMenuState`]); `0.0` makes the popup exactly its
//!   widest row, the way agg-sharp menus that reset `row.MinimumSize` to
//!   `(0, …)` size themselves (`GridOptionsPanel.ShowGridOptions`).  The one
//!   value covers the whole cascade: every submenu level uses the same floor
//!   as the root.
//!
//! Text is measured exactly as the row's [`crate::widgets::label::Label`]
//! measures it at paint time (system font override and font-size scale
//! included), so the fitted panel always holds the text it paints.  Widget
//! rows ([`super::MenuItem::widget_row`]) have no text and contribute only the
//! floor.  `geometry.rs` consumes the width through
//! [`super::geometry::stack_layout_with_width`]; `state.rs` stores the policy
//! and the measuring font so hit-testing and painting agree.

use std::sync::Arc;

use crate::text::{measure_advance, Font};

use super::geometry::{MenuMetrics, MENU_W};
use super::model::{MenuEntry, MenuItem};
use super::paint::MenuStyle;
use super::style::ShortcutFormat;

/// Gap between a row's label and its shortcut in a fitted popup (logical px).
pub const SHORTCUT_GAP: f64 = 20.0;
/// Narrowest fitted popup at desktop metrics (logical px).
pub const FIT_MIN_W: f64 = 156.0;

/// How wide a popup menu is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuWidth {
    /// Every popup is [`super::MENU_W`] wide (touch-grown on touch devices).
    #[default]
    Fixed,
    /// Each popup is as wide as its widest row's natural width.
    FitContent,
}

/// The font a fitted popup measures its rows with: the same font, size and
/// [`MenuStyle`] the popup paints with.
#[derive(Clone)]
pub struct FitMeasure {
    pub font: Arc<Font>,
    pub font_size: f64,
    pub label_x: f64,
    pub shortcut_right: f64,
    pub shortcut_format: ShortcutFormat,
}

impl std::fmt::Debug for FitMeasure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FitMeasure")
            .field("font_size", &self.font_size)
            .field("label_x", &self.label_x)
            .field("shortcut_right", &self.shortcut_right)
            .finish_non_exhaustive()
    }
}

impl FitMeasure {
    pub fn new(font: Arc<Font>, font_size: f64, style: &MenuStyle) -> Self {
        Self {
            font,
            font_size,
            label_x: style.label_x,
            shortcut_right: style.shortcut_right,
            shortcut_format: style.shortcut_format,
        }
    }

    /// Advance width of `text` as the row `Label` will measure it.
    fn text_width(&self, text: &str) -> f64 {
        let font =
            crate::font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font));
        let size = self.font_size * crate::font_settings::current_font_size_scale();
        measure_advance(&font, text, size)
    }

    /// Natural width of one item row: label, optional shortcut and padding.
    /// Widget rows have no text and report `0.0` (the popup floor applies).
    pub fn row_natural_width(&self, item: &MenuItem) -> f64 {
        if item.widget_row.is_some() {
            return 0.0;
        }
        let platform = crate::platform::current_platform();
        let label_w = self.text_width(&item.label);
        let shortcut_w = item
            .shortcut_text_formatted(platform, &self.font, self.shortcut_format)
            .map_or(0.0, |text| SHORTCUT_GAP + self.text_width(&text));
        self.label_x + label_w + shortcut_w + self.shortcut_right
    }

    /// Width of a fitted popup holding `items`: the widest row's natural
    /// width, rounded up to a whole logical px so no glyph is clipped, and
    /// never below the (touch-grown) [`FIT_MIN_W`] floor.
    pub fn popup_width(&self, items: &[MenuEntry], m: &MenuMetrics) -> f64 {
        self.popup_width_with_min(items, m, FIT_MIN_W)
    }

    /// [`Self::popup_width`] with a caller-chosen desktop floor `min_w`
    /// (logical px, touch-grown like [`FIT_MIN_W`]).  `0.0` means no floor:
    /// the popup is the widest row rounded up.  Negative or NaN floors count
    /// as `0.0`.
    pub fn popup_width_with_min(&self, items: &[MenuEntry], m: &MenuMetrics, min_w: f64) -> f64 {
        let widest = items
            .iter()
            .filter_map(|entry| match entry {
                MenuEntry::Item(item) => Some(self.row_natural_width(item)),
                MenuEntry::Separator => None,
            })
            .fold(0.0_f64, f64::max);
        let min_w = if min_w > 0.0 { min_w } else { 0.0 };
        let floor = min_w * (m.menu_w / MENU_W);
        widest.ceil().max(floor)
    }
}
