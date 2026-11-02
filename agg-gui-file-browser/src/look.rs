//! How a file browser looks: its colours and sizes ([`BrowserStyle`]), the icons it draws
//! ([`BrowserIcon`], drawn as an app's image or else a Font Awesome glyph), and the names its
//! widgets carry ([`BrowserNames`]), all handed over by one [`BrowserLook`] per browser.
//!
//! The defaults are agg-gui's own: [`DefaultLook`] reads the current [`Visuals`] each time its
//! style is asked for (so a theme switch shows on the next build) and draws every icon as its
//! Font Awesome glyph. An app with its own look implements [`BrowserLook`]: its colours and sizes
//! through [`BrowserStyle`], its artwork through [`BrowserLook::icon`], which the widgets draw
//! centred on whole device pixels at the size they ask for.
//!
//! The widget names default to the long-standing names automation scripts, tests and help
//! articles find the browser's buttons by ("Library Up Button", "Bread Crumb Button {name}"),
//! so a browser keeps them unless its app chooses others.

use std::sync::Arc;

use agg_gui::icon_image::IconImage;
use agg_gui::theme::{current_visuals, Visuals};
use agg_gui::{Color, Size};
use agg_gui_files::FolderLink;

/// The font size the browser's text takes when agg-gui has no default set.
const BUILTIN_FONT_SIZE: f64 = 14.0;
/// The size of a tile's name: 9 pt at 96 pixels to the inch.
const TILE_FONT_SIZE: f64 = 12.0;

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
            BrowserIcon::Item => '\u{F1B2}',       // cube
            BrowserIcon::Folder => '\u{F07B}',     // folder
            BrowserIcon::Generating => '\u{F252}', // hourglass-half
            BrowserIcon::Warning => '\u{F071}',    // exclamation-triangle
            BrowserIcon::Overflow => '\u{F142}',   // ellipsis-v
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
            text_color: visuals.text_color,
            light_text_color: visuals.text_dim,
            background: visuals.window_fill,
            border_color: visuals.widget_stroke,
            accent: visuals.accent,
            selection_fill: visuals.accent_focus,
            hover_fill: visuals.widget_bg_hovered,
            thumbnail_background: visuals.widget_bg,
            splitter_background: visuals.separator,
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

    /// Changes whenever the look's art changes (an app swapping its icon files): a widget that
    /// keeps an image it was given asks again when this differs from what it was built with.
    /// The default never changes.
    fn icon_generation(&self) -> u64 {
        0
    }

    /// The name a widget reports for `which` as its `icon` property (an app's icon file, say).
    fn icon_name(&self, which: BrowserIcon) -> String {
        which.key().to_string()
    }

    /// The picture of the folder `link` opens, `side` units square; `None` draws the folder
    /// icon.
    fn folder_tile(&self, link: &dyn FolderLink, side: f64) -> Option<IconImage> {
        let _ = (link, side);
        None
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

    fn names(&self) -> &BrowserNames {
        &self.names
    }
}

/// The face the default glyphs draw in: agg-gui's bundled Font Awesome 4.7.
pub fn glyph_face() -> Arc<agg_gui::text::Font> {
    agg_gui::icon_font::type_face()
}
