//! Popup-menu style: row geometry, panel chrome, width policy and shortcut
//! text format, plus the thread-local default every new popup starts from.
//!
//! [`MenuStyle`] is read by the painters in `paint.rs` / `widget/popup_paint.rs`
//! (chrome and row content), by `state.rs` (row height and width policy feed
//! the hit-test layout in `geometry.rs`), and by `fit_width.rs` / the row label
//! cache (shortcut text format).  `MenuStyle::default()` is the crate's
//! built-in look; a host that wants a different look everywhere — e.g. an
//! agg-sharp `PopupMenu` look with taller rows, a flat outlined panel and
//! content-fitted width — installs it once with [`set_menu_style`], and every
//! [`super::PopupMenu`] created afterwards (context menus, menu bars, the text
//! editing menus) starts from it.  A single popup can still be restyled with
//! [`super::PopupMenu::set_style`] / [`super::MenuBar::with_menu_style`].
//!
//! [`SelectionColumn`] is the opt-in agg-sharp check / radio layout: the mark
//! in its own leading column and the row's icon beside it.  [`row_icon_x`] and
//! [`row_label_x`] resolve where a given row's icon and label go, for the
//! painter (`widget/popup_paint.rs`) and the fitted width (`fit_width.rs`).

use std::cell::RefCell;

use crate::color::Color;

use super::fit_width::{MenuWidth, FIT_MIN_W};
use super::geometry::ROW_H;
use super::model::{MenuItem, MenuSelection};

/// How a row's keyboard shortcut is spelled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShortcutFormat {
    /// Native per platform: Apple's glyphs on macOS (`⇧⌘Z`, spelled out as
    /// `Cmd+Shift+Z` when the font lacks them), `Ctrl+Shift+Z` elsewhere.
    #[default]
    Platform,
    /// `Ctrl+Shift+Z` text on every platform, macOS included — what
    /// agg-sharp's `PopupMenu` shows (it draws the declared shortcut string
    /// verbatim).  Free-form shortcut text that does not parse is shown as
    /// declared, as with [`Self::Platform`].
    Plain,
}

/// Style values shared between the bar and popup painters and the popup
/// layout.
///
/// Every chrome indicator (submenu chevron, check mark, radio dot) is painted
/// as vector primitives by the menu widget, so the menu renders the same on
/// every host regardless of which icon font (if any) it bundles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuStyle {
    /// Corner radius of the panel (logical px).
    pub radius: f64,
    /// Offset of the drop shadow from the panel (Y-up logical px).
    pub shadow_offset: (f64, f64),
    /// Opacity of the drop shadow.
    pub shadow_alpha: f32,
    /// Whether the panel paints its drop shadow at all.
    pub shadow: bool,
    pub pad_x: f64,
    /// Left edge of the row's icon / check slot, from the row's left edge.
    pub icon_x: f64,
    /// Left edge of the row's label, from the row's left edge.  The gap
    /// between a 16 px icon and the label is `label_x - icon_x - 16`.
    pub label_x: f64,
    /// Distance from the row's right edge to the shortcut text's right edge.
    pub shortcut_right: f64,
    /// Height of an item row at desktop metrics (logical px).  Touch devices
    /// still floor it at the touch minimum.  Separators and widget rows keep
    /// their own heights.
    pub row_h: f64,
    /// Panel fill; `None` uses the theme's `panel_fill`.
    pub background: Option<Color>,
    /// Panel outline colour; `None` uses the theme's `widget_stroke`.
    pub border_color: Option<Color>,
    /// Panel outline width (logical px); `0.0` paints no outline.
    pub border_width: f64,
    /// Popup width policy (fixed [`super::MENU_W`] or fitted to content).
    pub width: MenuWidth,
    /// Narrowest a [`MenuWidth::FitContent`] panel may be (desktop logical
    /// px, touch-grown); `0.0` makes each panel exactly its widest row.
    pub min_width: f64,
    /// How shortcut text is spelled.
    pub shortcut_format: ShortcutFormat,
    /// Leading column for check and radio marks; `None` (the default)
    /// keeps the single icon column, where a mark shares the icon slot or
    /// moves to the row's right end when the row has an icon.
    pub selection_column: Option<SelectionColumn>,
}

/// A leading column for check and radio marks beside the icon column, as
/// agg-sharp's `CheckboxMenuItem` / `RadioMenuItem` draw them: the mark in
/// the row's gutter, then the row's icon, then its label.
///
/// With a column set, every [`MenuSelection::Radio`] row shows a circle
/// (hollow, with a centre dot when selected) and every selected
/// [`MenuSelection::Check`] row a check mark, centred at [`Self::mark_x`].
/// A check or radio row's icon moves to [`Self::icon_x`] and, when it has
/// one, its label to [`Self::icon_label_x`]; a check or radio row without
/// an icon keeps [`MenuStyle::label_x`], and rows with no selection keep the
/// style's `icon_x` / `label_x`.  Colour-swatch rows keep their own
/// selection outline and are not affected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionColumn {
    /// Centre of the mark, from the row's left edge (logical px).
    pub mark_x: f64,
    /// Left edge of a check / radio row's icon, from the row's left edge.
    pub icon_x: f64,
    /// Left edge of the label of a check / radio row that has an icon.
    pub icon_label_x: f64,
}

/// The column `item` paints its check / radio mark in, if any: `column` is
/// set, the row has a selection and is not a colour swatch.
pub fn row_selection_column(
    column: Option<SelectionColumn>,
    item: &MenuItem,
) -> Option<SelectionColumn> {
    column.filter(|_| item.selection != MenuSelection::None && item.swatch.is_none())
}

/// Left edge of `item`'s icon under `style`.
pub fn row_icon_x(style: &MenuStyle, item: &MenuItem) -> f64 {
    row_selection_column(style.selection_column, item).map_or(style.icon_x, |c| c.icon_x)
}

/// Left edge of `item`'s label for a menu whose default label edge is
/// `label_x` and whose selection column is `column`.
pub fn row_label_x(label_x: f64, column: Option<SelectionColumn>, item: &MenuItem) -> f64 {
    match row_selection_column(column, item) {
        Some(c) if item.has_leading_icon() => c.icon_label_x,
        _ => label_x,
    }
}

impl Default for MenuStyle {
    fn default() -> Self {
        Self {
            radius: 5.0,
            shadow_offset: (5.0, -5.0),
            shadow_alpha: 0.22,
            shadow: true,
            pad_x: 8.0,
            icon_x: 14.0,
            label_x: 32.0,
            shortcut_right: 28.0,
            row_h: ROW_H,
            background: None,
            border_color: None,
            border_width: 1.0,
            width: MenuWidth::Fixed,
            min_width: FIT_MIN_W,
            shortcut_format: ShortcutFormat::Platform,
            selection_column: None,
        }
    }
}

thread_local! {
    static CURRENT_MENU_STYLE: RefCell<MenuStyle> = RefCell::new(MenuStyle::default());
}

/// The style new popups start from on this thread (the UI thread):
/// [`MenuStyle::default`] unless a host called [`set_menu_style`].
pub fn current_menu_style() -> MenuStyle {
    CURRENT_MENU_STYLE.with(|c| *c.borrow())
}

/// Replace the style every popup created afterwards on this thread starts
/// from.  Popups that already exist keep their style.
pub fn set_menu_style(style: MenuStyle) {
    CURRENT_MENU_STYLE.with(|c| *c.borrow_mut() = style);
}

/// Return the thread's default popup style to [`MenuStyle::default`].
pub fn reset_menu_style() {
    set_menu_style(MenuStyle::default());
}
