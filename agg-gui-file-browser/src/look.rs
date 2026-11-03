//! How a file browser looks: its colours and sizes ([`BrowserStyle`]), the icons it draws
//! ([`BrowserIcon`], drawn as an app's image or else a Font Awesome glyph), and the names its
//! widgets carry ([`BrowserNames`]), all handed over by one [`BrowserLook`] per browser.
//!
//! The defaults are agg-gui's own: [`DefaultLook`] reads the current [`Visuals`] each time its
//! style is asked for (so a theme switch shows on the next build) and draws every icon as its
//! Font Awesome glyph. An app with its own look implements [`BrowserLook`]: its colours and sizes
//! through [`BrowserStyle`], its artwork through [`BrowserLook::icon`], which the widgets draw
//! centred on whole device pixels at the size they ask for. A look may also build two of the
//! search's pieces as its app's own widgets ([`BrowserLook::section_toggle`],
//! [`BrowserLook::working_indicator`]), so a search shows as the app's other sections do, and
//! the navigation bar's menu buttons, overflow button and search field as its own widgets
//! ([`BrowserLook::menu_button`], [`BrowserLook::overflow_button`],
//! [`BrowserLook::search_field`], [`BrowserLook::clear_search_button`]), so a browser's bar looks
//! and opens its menus as the app's other bars do while the browser keeps what they show and do.
//! Over the list a look may put a banner of its own ([`BrowserLook::list_banner`]) and draw the
//! folder's header markdown with the app's markdown widget ([`BrowserLook::header_markdown`]);
//! it names the view modes the browser reports ([`BrowserLook::view_mode_name`]) and sets the
//! dialog page bar's padding and rule ([`BrowserLook::page_nav_bar_padding`],
//! [`BrowserLook::page_nav_bar_rule`]).
//!
//! The widget names default to the long-standing names automation scripts, tests and help
//! articles find the browser's buttons by ("Library Up Button", "Bread Crumb Button {name}"),
//! so a browser keeps them unless its app chooses others.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::icon_image::IconImage;
use agg_gui::text::Font;
use agg_gui::theme::{current_visuals, Visuals};
use agg_gui::{Color, Insets, Size, TextField, Widget};
use agg_gui_files::FolderLink;

use crate::memory::ViewMode;
use crate::nav::menu_button::NavMenuRows;
use crate::nav::overflow::OverflowRows;
use crate::state::{BrowserState, ListingMessage};

/// The font size the browser's text takes when agg-gui has no default set.
const BUILTIN_FONT_SIZE: f64 = 14.0;
/// The size of a tile's name: 9 pt at 96 pixels to the inch.
const TILE_FONT_SIZE: f64 = 12.0;
/// The size of the history notice's text: 10 pt at 96 pixels to the inch.
const NOTICE_FONT_SIZE: f64 = 10.0 / 72.0 * 96.0;

/// An icon a browser widget draws. Each has a Font Awesome glyph it falls back to
/// ([`BrowserIcon::glyph`]) and a name the widgets report for inspection
/// ([`BrowserIcon::key`], replaced by [`BrowserLook::icon_name`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BrowserIcon {
    /// The up button: back to the folder above.
    Up,
    /// The view options menu button.
    ViewOptions,
    /// The sorting menu button.
    Sort,
    /// Take back the last change.
    Undo,
    /// Make the change taken back again.
    Redo,
    /// An item with no picture of its own.
    Item,
    /// A folder with no picture of its own.
    Folder,
    /// An item whose picture is still being made.
    Generating,
    /// A message row that warns.
    Warning,
    /// A row's or the browser's overflow menu.
    Overflow,
    /// The search box's clear button.
    ClearSearch,
}

impl BrowserIcon {
    /// Every icon, in declaration order.
    pub const ALL: &'static [BrowserIcon] = &[
        BrowserIcon::Up,
        BrowserIcon::ViewOptions,
        BrowserIcon::Sort,
        BrowserIcon::Undo,
        BrowserIcon::Redo,
        BrowserIcon::Item,
        BrowserIcon::Folder,
        BrowserIcon::Generating,
        BrowserIcon::Warning,
        BrowserIcon::Overflow,
        BrowserIcon::ClearSearch,
    ];

    /// The Font Awesome 4.7 glyph drawn when the look has no image for the icon (agg-gui's
    /// bundled face, `agg_gui::icon_font::type_face`).
    pub fn glyph(self) -> char {
        match self {
            BrowserIcon::Up => '\u{F060}',          // arrow-left
            BrowserIcon::ViewOptions => '\u{F00B}', // th-list
            BrowserIcon::Sort => '\u{F0DC}',        // sort
            BrowserIcon::Undo => agg_gui::icon_font::UNDO,
            BrowserIcon::Redo => agg_gui::icon_font::REDO,
            BrowserIcon::Item => '\u{F1B2}',        // cube
            BrowserIcon::Folder => '\u{F07B}',      // folder
            BrowserIcon::Generating => '\u{F252}',  // hourglass-half
            BrowserIcon::Warning => '\u{F071}',     // exclamation-triangle
            BrowserIcon::Overflow => '\u{F142}',    // ellipsis-v
            BrowserIcon::ClearSearch => '\u{F00D}', // times
        }
    }

    /// The icon's name, reported as a widget's `icon` property unless the look names it.
    pub fn key(self) -> &'static str {
        match self {
            BrowserIcon::Up => "up",
            BrowserIcon::ViewOptions => "view-options",
            BrowserIcon::Sort => "sort",
            BrowserIcon::Undo => "undo",
            BrowserIcon::Redo => "redo",
            BrowserIcon::Item => "item",
            BrowserIcon::Folder => "folder",
            BrowserIcon::Generating => "generating",
            BrowserIcon::Warning => "warning",
            BrowserIcon::Overflow => "overflow",
            BrowserIcon::ClearSearch => "clear-search",
        }
    }
}

/// The colours and sizes a browser draws with. Sizes are in logical units.
#[derive(Clone, Debug, PartialEq)]
pub struct BrowserStyle {
    /// The size of a row's, a link's and a label's text.
    pub font_size: f64,
    /// The size of the name under an icon view's tile (9 pt).
    pub tile_font_size: f64,
    /// The size of the history notice's text and its button's word (10 pt).
    pub notice_font_size: f64,
    /// Text, links, separators and tinted icons.
    pub text_color: Color,
    /// Secondary text: a row's detail line, hints.
    pub light_text_color: Color,
    /// Behind the browser.
    pub background: Color,
    /// Borders and dividing lines.
    pub border_color: Color,
    /// The accent: a folder tile's colour, a focused control.
    pub accent: Color,
    /// Behind a selected row or tile.
    pub selection_fill: Color,
    /// Behind a button or row under the pointer.
    pub hover_fill: Color,
    /// Behind an unselected row's or tile's picture.
    pub thumbnail_background: Color,
    /// The splitter between the folder tree and the list.
    pub splitter_background: Color,
    /// The rule under a search section's header.
    pub section_rule: Color,
    /// The side of a square toolbar button (the up button, the menu buttons).
    pub button_height: f64,
    /// The corners of a tile and of its selection and hover highlights.
    pub corner_radius: f64,
}

impl BrowserStyle {
    /// The style agg-gui's `visuals` give, with agg-gui's default font size.
    pub fn from_visuals(visuals: &Visuals) -> Self {
        Self {
            font_size: agg_gui::font_settings::default_font_size_or(BUILTIN_FONT_SIZE),
            tile_font_size: TILE_FONT_SIZE,
            notice_font_size: NOTICE_FONT_SIZE,
            text_color: visuals.text_color,
            light_text_color: visuals.text_dim,
            background: visuals.window_fill,
            border_color: visuals.widget_stroke,
            accent: visuals.accent,
            selection_fill: visuals.accent_focus,
            hover_fill: visuals.widget_bg_hovered,
            thumbnail_background: visuals.widget_bg,
            splitter_background: visuals.separator,
            section_rule: visuals.separator,
            button_height: 32.0,
            corner_radius: 3.0,
        }
    }
}

impl Default for BrowserStyle {
    /// The style of agg-gui's current theme.
    fn default() -> Self {
        Self::from_visuals(&current_visuals())
    }
}

/// The names a browser's widgets carry, which `find_widget_by_id`, automation scripts and help
/// articles find them by. A name with `{name}` in it is a template the item's or folder's name
/// fills.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserNames {
    /// The breadcrumb trail.
    pub bread_crumb: String,
    /// One folder's link in the breadcrumb trail (template).
    pub bread_crumb_button: String,
    /// The up button at the start of the trail.
    pub up_button: String,
    /// The list of rows or tiles.
    pub rows: String,
    /// The folder's header text above the list.
    pub header_markdown: String,
    /// The view options button.
    pub view_options_button: String,
    /// The sorting button.
    pub sort_button: String,
    /// The Undo button.
    pub undo_button: String,
    /// The Redo button.
    pub redo_button: String,
    /// The host of the list's right-click menus.
    pub context_menu: String,
    /// The browser's overflow menu button.
    pub overflow_menu: String,
    /// An item's row or tile (template).
    pub item_row: String,
    /// A folder's row or tile (template).
    pub folder_row: String,
    /// The list's area around its rows or tiles, where a right press on empty space lands.
    pub list_area: String,
    /// What the list shows in place of a folder whose listing is only a message.
    pub folder_message: String,
    /// The message's wrapped text.
    pub folder_message_text: String,
    /// The Sign In button under a folder refused for the sign-in.
    pub sign_in_button: String,
    /// The "Searching for ..." line over a search's results.
    pub search_status: String,
    /// A search section's header (template; its expand toggle is the header's name and
    /// " Toggle").
    pub search_section: String,
    /// The folder tree.
    pub tree: String,
    /// One folder's row in the folder tree (template).
    pub tree_row: String,
    /// The search box.
    pub search_panel: String,
    /// The search box's text field.
    pub search_input: String,
    /// The search box's clear button.
    pub close_search: String,
    /// The split between the folder tree and the list.
    pub splitter: String,
    /// The provider message under a list-only browser's list.
    pub provider_message: String,
    /// The list-only browser's second bar (its search box, view and sorting buttons).
    pub folders_toolbar: String,
    /// A dialog page's nav bar (its breadcrumbs, view and sorting buttons).
    pub page_nav_bar: String,
    /// The history's notice ("Moved 3 items to Brackets").
    pub notice: String,
    /// The notice's Undo or Redo button.
    pub notice_button: String,
    /// A dialog page's accepting button (Save, Move, Open, a question's confirming button).
    pub dialog_confirm: String,
    /// A dialog page's going-back button (Cancel, a question's Cancel).
    pub dialog_cancel: String,
    /// The Save page's name field.
    pub save_name_field: String,
    /// The Save page's note (why Save can't run, or the question asked).
    pub save_note: String,
    /// The Save page's Create Folder button.
    pub create_folder_button: String,
    /// The Save page's field for a new folder's name, when it asks in the page.
    pub folder_name_field: String,
}

impl Default for BrowserNames {
    /// The long-standing names (module docs).
    fn default() -> Self {
        Self {
            bread_crumb: "FolderBreadCrumbWidget".into(),
            bread_crumb_button: "Bread Crumb Button {name}".into(),
            up_button: "Library Up Button".into(),
            rows: "Library ListView".into(),
            header_markdown: "Library Header Markdown".into(),
            view_options_button: "Library View Options Button".into(),
            sort_button: "Print Library View Options".into(),
            undo_button: "Library Undo Button".into(),
            redo_button: "Library Redo Button".into(),
            context_menu: "Library Context Menu".into(),
            overflow_menu: "Print Library Overflow Menu".into(),
            item_row: "Row Item {name}".into(),
            folder_row: "{name} Row Item Collection".into(),
            list_area: "LibraryView".into(),
            folder_message: "Library Folder Message".into(),
            folder_message_text: "Library Folder Message Text".into(),
            sign_in_button: "Library Folder Sign In Button".into(),
            search_status: "Search Status".into(),
            search_section: "Search Section {name}".into(),
            tree: "Library TreeView".into(),
            tree_row: "{name} Tree Node".into(),
            search_panel: "Library Search Panel".into(),
            search_input: "Library Search Input".into(),
            close_search: "Close Search".into(),
            splitter: "Library Splitter".into(),
            provider_message: "Provider Message".into(),
            folders_toolbar: "Folders Toolbar".into(),
            page_nav_bar: "Library Browser Nav Bar".into(),
            notice: "Library Notice".into(),
            notice_button: "Library Notice Button".into(),
            dialog_confirm: "Browser Dialog Confirm Button".into(),
            dialog_cancel: "Browser Dialog Cancel Button".into(),
            save_name_field: "Save Name Field".into(),
            save_note: "Save As Note".into(),
            create_folder_button: "Create Folder In Button".into(),
            folder_name_field: "Save As Folder Name Field".into(),
        }
    }
}

impl BrowserNames {
    /// The breadcrumb link of the folder named `name`.
    pub fn bread_crumb_button_for(&self, name: &str) -> String {
        self.bread_crumb_button.replace("{name}", name)
    }

    /// The row or tile of the item named `name`.
    pub fn item_row_for(&self, name: &str) -> String {
        self.item_row.replace("{name}", name)
    }

    /// The header of the search section named `name`.
    pub fn search_section_for(&self, name: &str) -> String {
        self.search_section.replace("{name}", name)
    }

    /// The folder tree's row of the folder named `name`.
    pub fn tree_row_for(&self, name: &str) -> String {
        self.tree_row.replace("{name}", name)
    }

    /// The row or tile of the folder named `name`.
    pub fn folder_row_for(&self, name: &str) -> String {
        self.folder_row.replace("{name}", name)
    }
}

/// A browser's look: its style, icons and widget names ([module docs](self)).
pub trait BrowserLook {
    /// The colours and sizes to draw with, asked for when a widget is built.
    fn style(&self) -> BrowserStyle;

    /// The image for `which`, drawn `logical` in size, dimmed by the look when not `enabled`;
    /// `None` draws the icon's Font Awesome glyph. Asked again when the device scale changes, so
    /// a look may hand back a raster made for the current scale.
    fn icon(&self, which: BrowserIcon, logical: Size, enabled: bool) -> Option<IconImage> {
        let _ = (which, logical, enabled);
        None
    }

    /// The image for `which` drawn `logical` in size in `tint` (a folder's accent, a part's text
    /// colour); `None` draws the icon's Font Awesome glyph in `tint`. The default is
    /// [`Self::icon`] enabled, for a look whose art keeps its own colours.
    fn tinted_icon(&self, which: BrowserIcon, logical: Size, tint: Color) -> Option<IconImage> {
        let _ = tint;
        self.icon(which, logical, true)
    }

    /// The image an item names by an icon key (a folder link's `micro_icon` or `tile_icon`),
    /// drawn `logical` in size in `tint`; `None` (the default) when the look has no art for it.
    fn icon_for_key(&self, key: &str, logical: Size, tint: Color) -> Option<IconImage> {
        let _ = (key, logical, tint);
        None
    }

    /// The button a folder's message offers (its Sign In), titled `title`, taking a press while
    /// `enabled` says so and running `press`; the widget carries `name` itself, so what finds it
    /// by name reads whether it takes a press. `None` (the default) is agg-gui's `Button` in the
    /// browser's font.
    fn message_button(
        &self,
        title: &str,
        name: &str,
        enabled: Rc<dyn Fn() -> bool>,
        press: Rc<dyn Fn()>,
    ) -> Option<Box<dyn Widget>> {
        let _ = (title, name, enabled, press);
        None
    }

    /// The icon of a folder tree row whose link names no icon key (`micro_icon`), `logical` in
    /// size in `tint`; `None` (the default) draws no icon there.
    fn unkeyed_folder_icon(&self, logical: Size, tint: Color) -> Option<IconImage> {
        let _ = (logical, tint);
        None
    }

    /// Changes whenever the look's art changes (an app swapping its icon files): a widget that
    /// keeps an image it was given asks again when this differs from what it was built with.
    /// The default never changes.
    fn icon_generation(&self) -> u64 {
        0
    }

    /// Shows `hint` while the pointer is over a button that carries one (an app's status bar:
    /// "⌘Z" over Undo), or clears it (`None`) when the pointer leaves. The default shows nothing.
    fn show_hint(&self, hint: Option<&str>) {
        let _ = hint;
    }

    /// The name a widget reports for `which` as its `icon` property (an app's icon file, say).
    fn icon_name(&self, which: BrowserIcon) -> String {
        which.key().to_string()
    }

    /// The picture of the folder `link` opens, `side` units square, asked before the link's own
    /// tile image (`FolderLink::tile_icon`), so a look may draw that its own way; `None` draws
    /// the link's tile image, else the folder icon.
    fn folder_tile(&self, link: &dyn FolderLink, side: f64) -> Option<IconImage> {
        let _ = (link, side);
        None
    }

    /// The name a row reports as its `thumbnail` for the folder `link` opens (the look's own
    /// file for its folder tile, say); `None` (the default) reports the link's tile image key,
    /// else the folder icon's name.
    fn folder_tile_name(&self, link: &dyn FolderLink) -> Option<String> {
        let _ = link;
        None
    }

    /// The expand toggle of the search section header for the place `name` (open while
    /// `open`), carrying the widget id `id`, its text in `font`; `None` (the default) draws the
    /// browser's own, a caret and the name. Clicks anywhere on the header are the header's, so
    /// an app's toggle needs no handling of its own.
    fn section_toggle(
        &self,
        id: &str,
        name: &str,
        open: bool,
        font: &Arc<Font>,
    ) -> Option<Box<dyn Widget>> {
        let _ = (id, name, open, font);
        None
    }

    /// The indicator a search shows while it works (after its "Searching for" line, and after a
    /// section still searching); `None` (the default) draws agg-gui's small spinner in the
    /// style's accent.
    fn working_indicator(&self) -> Option<Box<dyn Widget>> {
        None
    }

    /// A navigation bar menu button drawn the app's way: `which` is `BrowserIcon::ViewOptions`
    /// or `BrowserIcon::Sort`, the widget carries `name` and `tooltip`, and its menu shows `rows`,
    /// asked for each time it opens (picking a row runs the row's `run`). `None` (the default) is
    /// the browser's own `NavMenuButton`.
    fn menu_button(
        &self,
        which: BrowserIcon,
        name: &str,
        tooltip: &str,
        rows: NavMenuRows,
        font: &Arc<Font>,
    ) -> Option<Box<dyn Widget>> {
        let _ = (which, name, tooltip, rows, font);
        None
    }

    /// The navigation bar's overflow button drawn the app's way, carrying `name`; its menu shows
    /// `rows`, the open folder's whole menu as the browser's own overflow menu shows it (built-ins
    /// resolved, greyed when they can't run), asked for each time it opens. `None` (the default)
    /// is the browser's own `OverflowButton`.
    fn overflow_button(
        &self,
        name: &str,
        rows: OverflowRows,
        font: &Arc<Font>,
    ) -> Option<Box<dyn Widget>> {
        let _ = (name, rows, font);
        None
    }

    /// The search box's text field as the app styles its fields, before the box gives it its
    /// placeholder, text and handlers. `None` (the default) is agg-gui's `TextField` in `font`
    /// at the style's font size.
    fn search_field(&self, font: &Arc<Font>) -> Option<TextField> {
        let _ = font;
        None
    }

    /// The search box's clear button drawn the app's way, carrying `name` and `tooltip` and
    /// running `clear` when clicked. `None` (the default) is a `BrowserIconButton` showing
    /// `BrowserIcon::ClearSearch`.
    fn clear_search_button(
        &self,
        name: &str,
        tooltip: &str,
        clear: Rc<dyn Fn()>,
        font: &Arc<Font>,
    ) -> Option<Box<dyn Widget>> {
        let _ = (name, tooltip, clear, font);
        None
    }

    /// The app's own widget for a folder whose listing is only `message` (loading, or why it
    /// could not be listed), shown in the list's place in `font`; the list has already numbered
    /// its rows as none. `None` (the default) shows the browser's own `FolderMessage`.
    fn folder_message(
        &self,
        state: &Rc<RefCell<BrowserState>>,
        message: &ListingMessage,
        font: &Arc<Font>,
    ) -> Option<Box<dyn Widget>> {
        let _ = (state, message, font);
        None
    }

    /// A banner over the list of what `state` shows (a hint about the folder showing), asked
    /// for again each time the list shows its content anew (`shown_anew`: a folder opened, a
    /// search run) and when the folder's header text changes (`shown_anew` false). It reads
    /// what it shows at its own layouts, taking no room when it has nothing to say. `None` (the
    /// default) puts nothing there.
    fn list_banner(
        &self,
        state: &Rc<RefCell<BrowserState>>,
        shown_anew: bool,
        font: &Arc<Font>,
    ) -> Option<Box<dyn Widget>> {
        let _ = (state, shown_anew, font);
        None
    }

    /// The showing folder's header markdown `text` (never empty) drawn the app's way, over the
    /// list. `None` (the default) is agg-gui's `MarkdownView` in `font`.
    fn header_markdown(&self, text: &str, font: &Arc<Font>) -> Option<Box<dyn Widget>> {
        let _ = (text, font);
        None
    }

    /// The name the browser reports for `mode` (its `view_mode` property). The default is the
    /// mode's Rust name (`Rows`, `Icons70`).
    fn view_mode_name(&self, mode: ViewMode) -> String {
        format!("{mode:?}")
    }

    /// The padding inside a dialog page's nav bar (`BrowserPage`). The default is 3 left and
    /// right, 1 top and bottom.
    fn page_nav_bar_padding(&self) -> Insets {
        Insets::symmetric(3.0, 1.0)
    }

    /// Whether a dialog page's nav bar draws its one-pixel rule along its bottom (the default).
    fn page_nav_bar_rule(&self) -> bool {
        true
    }

    /// The names the browser's widgets carry.
    fn names(&self) -> &BrowserNames;
}

/// agg-gui's own look: the current theme's colours and Font Awesome glyphs.
#[derive(Clone, Debug, Default)]
pub struct DefaultLook {
    names: BrowserNames,
}

impl DefaultLook {
    /// The default look with its widgets named `names`.
    pub fn with_names(names: BrowserNames) -> Self {
        Self { names }
    }
}

impl BrowserLook for DefaultLook {
    fn style(&self) -> BrowserStyle {
        BrowserStyle::default()
    }

    /// The "folder" key (a link's `micro_icon`, `BrowserIcon::Folder`'s key) draws a folder in
    /// `tint`, so a folder tree's rows look like folders; every other key has no art.
    fn icon_for_key(&self, key: &str, logical: Size, tint: Color) -> Option<IconImage> {
        if key != BrowserIcon::Folder.key() {
            return None;
        }
        IconImage::from_svg_data(folder_svg(tint).as_bytes(), logical).ok()
    }

    /// Every folder row draws a folder, whether or not its link names an icon.
    fn unkeyed_folder_icon(&self, logical: Size, tint: Color) -> Option<IconImage> {
        IconImage::from_svg_data(folder_svg(tint).as_bytes(), logical).ok()
    }

    fn names(&self) -> &BrowserNames {
        &self.names
    }
}

/// A folder silhouette (a tab over a body) filled with `tint`, 20 units square.
fn folder_svg(tint: Color) -> String {
    // Colour channels are 0..=1; the cast cannot overflow after the clamp.
    let channel = |v: f32| (f64::from(v).clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"20\" viewBox=\"0 0 20 20\">\
         <path fill=\"#{:02x}{:02x}{:02x}\" fill-opacity=\"{}\" \
         d=\"M2 4h6l2 2h8v10H2z\"/></svg>",
        channel(tint.r),
        channel(tint.g),
        channel(tint.b),
        f64::from(tint.a).clamp(0.0, 1.0)
    )
}

/// The face the default glyphs draw in: agg-gui's bundled Font Awesome 4.7.
pub fn glyph_face() -> Arc<agg_gui::text::Font> {
    agg_gui::icon_font::type_face()
}
