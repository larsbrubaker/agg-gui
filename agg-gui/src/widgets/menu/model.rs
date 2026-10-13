//! Data model for popup and menu-bar menus.
//!
//! Menus are application-owned item trees. The widget layer interprets this
//! model for painting, hit testing, keyboard navigation, and action dispatch.
//!
//! A row can also host an arbitrary widget ([`MenuItem::widget_row`]): the
//! model carries only the row's id and height (so the model stays plain,
//! `Send` data), and the widget itself is registered on the owning
//! [`super::PopupMenu`] with `set_row_widget` (see `menu/row_widgets.rs`).
//!
//! An ordinary row can also carry its own height ([`MenuItem::row_height`])
//! and its own icon slot ([`MenuIconSlot`]): a taller row with a larger
//! picture in a wider gutter (agg-sharp's `MenuItem.Height` /
//! `MenuItem.GutterWidth`, e.g. a recent-files row with a thumbnail).  It
//! stays an ordinary row: hovered, keyboard-selected, activated and
//! tooltipped like any other.

use crate::color::Color;
use crate::event::{Key, Modifiers};
use crate::geometry::Size;
use crate::icon_image::IconImage;
use crate::platform;

// Nearly every entry is an `Item`, so boxing it would add an allocation and
// an indirection per row without saving memory: the size gap is only to the
// rare data-less `Separator`.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum MenuEntry {
    Item(MenuItem),
    Separator,
}

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub label: String,
    pub icon: Option<char>,
    /// Image painted in the icon slot (e.g. an artwork icon ported from
    /// another app).  When `Some`, takes precedence over the `icon` glyph;
    /// a `swatch` still wins over both.  Drawn at its logical size,
    /// rasterised for the current device scale.  Set with
    /// [`MenuItem::image_icon`].
    pub icon_image: Option<IconImage>,
    /// Colour swatch painted in the icon slot.  When `Some`, takes
    /// precedence over `icon` and over the check / radio selection
    /// glyph — the popup paints a rounded filled rect in the icon
    /// column.  For radio-style rows the selected item gets a thin
    /// stroke around the swatch instead of the usual `radio_glyph`,
    /// so the colour itself remains the dominant cue.
    ///
    /// Intended for menus where each item represents a colour the
    /// user is picking (accent palette, brush colour, etc.).
    pub swatch: Option<Color>,
    /// The shortcut as declared (e.g. `"Ctrl+X"`).  Not drawn verbatim when it
    /// parses into [`Self::accelerator`]: the menu renders
    /// [`MenuItem::shortcut_text_for_font`] so a Mac shows `⌘X`.
    pub shortcut: Option<String>,
    pub accelerator: Option<MenuShortcut>,
    pub enabled: bool,
    pub selection: MenuSelection,
    pub action: Option<String>,
    pub submenu: Vec<MenuEntry>,
    pub close_on_activate: bool,
    /// When `Some`, this row hosts the widget registered under
    /// [`MenuWidgetRow::id`] on the owning `PopupMenu` instead of a label.
    /// Widget rows are never hovered, keyboard-selected or activated by
    /// the menu; pointer events inside them go to the widget.
    pub widget_row: Option<MenuWidgetRow>,
    /// Hover help for this row.  While the pointer rests on the row the
    /// owning `PopupMenu` hands it to the app-wide tooltip controller, so it
    /// appears after the normal hover delay, anchored at the pointer like
    /// every other tooltip.  Set with [`MenuItem::tooltip()`].
    pub tooltip: Option<String>,
    /// This row's height (logical px) in place of the style's
    /// [`super::MenuStyle::row_h`].  Touch devices still floor it at the
    /// touch minimum, as they do the style's.  `None` (the default) keeps
    /// the style's row height.  Ignored on a widget row, which has its own.
    /// Set with [`MenuItem::row_height()`].
    pub row_height: Option<f64>,
    /// This row's icon gutter and label edge in place of the style's icon
    /// column and `label_x`.  `None` (the default) keeps the style's.  Set
    /// with [`MenuItem::icon_slot()`].
    pub icon_slot: Option<MenuIconSlot>,
    /// The row's GUI-automation name in place of the `"{label} Menu Item"`
    /// convention (agg-sharp `MenuItemModel.AutomationName`, which
    /// `MenuModelPopupBuilder` sets as the row's `Name`).  `None` (the
    /// default) keeps the convention.  Set with
    /// [`MenuItem::with_automation_name()`]; read through
    /// [`menu_row_name`](super::menu_row_name).
    pub automation_name: Option<String>,
}

/// A row's own icon slot: a gutter of `gutter_width` starting `gutter_x`
/// from the row's left edge, with the row's image (or glyph) centred in it
/// at `image_size`, and the label starting at `label_x`.  Mirrors agg-sharp
/// `PopupMenu.MenuItem`, which centres its `Image` in its `GutterWidth`
/// and starts its content at its left `Padding`.  All values are logical px
/// from the row's left edge, except `label_right`: the room kept between
/// the label (or shortcut) and the row's right edge, in place of the
/// style's `shortcut_right`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuIconSlot {
    pub gutter_x: f64,
    pub gutter_width: f64,
    pub image_size: Size,
    pub label_x: f64,
    pub label_right: f64,
}

impl MenuIconSlot {
    /// Left edge of an `image_size` picture centred in the gutter, from the
    /// row's left edge.
    pub fn image_x(&self) -> f64 {
        self.gutter_x + (self.gutter_width - self.image_size.width) * 0.5
    }
}

/// A menu row that hosts a widget: the id it is registered under on the
/// owning `PopupMenu` and the row's height (logical px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuWidgetRow {
    pub id: usize,
    pub height: f64,
    /// Whether Up / Down step onto this row like onto a text row
    /// ([`MenuItem::keyboard_focusable`]). Off by default.
    pub keyboard_focus: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuSelection {
    None,
    Check { selected: bool },
    Radio { selected: bool },
}

impl MenuItem {
    pub fn action(label: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            icon_image: None,
            swatch: None,
            shortcut: None,
            accelerator: None,
            enabled: true,
            selection: MenuSelection::None,
            action: Some(action.into()),
            submenu: Vec::new(),
            close_on_activate: true,
            widget_row: None,
            tooltip: None,
            row_height: None,
            icon_slot: None,
            automation_name: None,
        }
    }

    pub fn submenu(label: impl Into<String>, submenu: Vec<MenuEntry>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            icon_image: None,
            swatch: None,
            shortcut: None,
            accelerator: None,
            enabled: true,
            selection: MenuSelection::None,
            action: None,
            submenu,
            close_on_activate: false,
            widget_row: None,
            tooltip: None,
            row_height: None,
            icon_slot: None,
            automation_name: None,
        }
    }

    /// A row of `height` logical px hosting the widget registered under
    /// `id` with `PopupMenu::set_row_widget`.  It has no label or action.
    pub fn widget_row(id: usize, height: f64) -> Self {
        Self {
            label: String::new(),
            icon: None,
            icon_image: None,
            swatch: None,
            shortcut: None,
            accelerator: None,
            enabled: true,
            selection: MenuSelection::None,
            action: None,
            submenu: Vec::new(),
            close_on_activate: false,
            widget_row: Some(MenuWidgetRow {
                id,
                height: height.max(0.0),
                keyboard_focus: false,
            }),
            tooltip: None,
            row_height: None,
            icon_slot: None,
            automation_name: None,
        }
    }

    /// Lets the keyboard reach a [`MenuItem::widget_row`] row: Up / Down step
    /// onto it, the widget gets [`Event::FocusGained`] / [`Event::FocusLost`]
    /// as the highlight arrives and leaves (it draws its own focus look; the
    /// menu paints none), and while it is highlighted Left, Right, Enter and
    /// Space go to its `on_event` first. A widget that consumes a key keeps
    /// it from the menu (Right would otherwise open a submenu, Enter
    /// activate a row). No effect on other rows.
    pub fn keyboard_focusable(mut self) -> Self {
        if let Some(row) = self.widget_row.as_mut() {
            row.keyboard_focus = true;
        }
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.selection = MenuSelection::Check { selected: checked };
        self
    }

    pub fn radio(mut self, selected: bool) -> Self {
        self.selection = MenuSelection::Radio { selected };
        self
    }

    pub fn keep_open(mut self) -> Self {
        self.close_on_activate = false;
        self
    }

    pub fn close_on_activate(mut self, close: bool) -> Self {
        self.close_on_activate = close;
        self
    }

    pub fn icon(mut self, icon: char) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Paint `image` in the icon slot instead of a glyph.  See
    /// [`MenuItem::icon_image`].
    pub fn image_icon(mut self, image: IconImage) -> Self {
        self.icon_image = Some(image);
        self
    }

    /// True when the row shows something in its leading icon slot
    /// (glyph, image or swatch).
    pub fn has_leading_icon(&self) -> bool {
        self.swatch.is_some() || self.icon_image.is_some() || self.icon.is_some()
    }

    /// Paint a colour swatch in the icon slot instead of a glyph.  See
    /// [`MenuItem::swatch`] for behaviour details.
    pub fn swatch(mut self, color: Color) -> Self {
        self.swatch = Some(color);
        self
    }

    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        let shortcut = shortcut.into();
        self.accelerator = MenuShortcut::parse(&shortcut);
        self.shortcut = Some(shortcut);
        self
    }

    pub fn accelerator(mut self, accelerator: MenuShortcut) -> Self {
        self.shortcut = Some(accelerator.display_text());
        self.accelerator = Some(accelerator);
        self
    }

    /// Give this row its own height (see the `row_height` field).
    pub fn row_height(mut self, height: f64) -> Self {
        self.row_height = Some(height);
        self
    }

    /// Give this row its own icon gutter and label edge (see
    /// [`MenuIconSlot`]).
    pub fn icon_slot(mut self, slot: MenuIconSlot) -> Self {
        self.icon_slot = Some(slot);
        self
    }

    /// Show `text` as hover help for this row (see the `tooltip` field).
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Name this row `name` for GUI automation (see the `automation_name`
    /// field), as C# `MenuItemModel.AutomationName` does.
    pub fn with_automation_name(mut self, name: impl Into<String>) -> Self {
        self.automation_name = Some(name.into());
        self
    }

    pub fn has_submenu(&self) -> bool {
        !self.submenu.is_empty()
    }
}

impl From<MenuItem> for MenuEntry {
    fn from(item: MenuItem) -> Self {
        Self::Item(item)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuShortcut {
    pub key: ShortcutKey,
    /// Portable command modifier: Ctrl on Windows/Linux, Cmd on macOS.
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutKey {
    Char(char),
    Insert,
    Delete,
    Backspace,
    Enter,
    Escape,
    Tab,
    Space,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
}

impl MenuShortcut {
    pub fn command_char(ch: char) -> Self {
        Self {
            key: ShortcutKey::Char(ch.to_ascii_uppercase()),
            command: true,
            shift: false,
            alt: false,
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let mut command = false;
        let mut shift = false;
        let mut alt = false;
        let mut key = None;
        for part in text.split('+') {
            let token = part.trim();
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" | "meta" => command = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                "insert" => key = Some(ShortcutKey::Insert),
                "delete" | "del" => key = Some(ShortcutKey::Delete),
                "backspace" => key = Some(ShortcutKey::Backspace),
                "enter" | "return" => key = Some(ShortcutKey::Enter),
                "esc" | "escape" => key = Some(ShortcutKey::Escape),
                "tab" => key = Some(ShortcutKey::Tab),
                "space" => key = Some(ShortcutKey::Space),
                "left" | "arrowleft" => key = Some(ShortcutKey::ArrowLeft),
                "right" | "arrowright" => key = Some(ShortcutKey::ArrowRight),
                "up" | "arrowup" => key = Some(ShortcutKey::ArrowUp),
                "down" | "arrowdown" => key = Some(ShortcutKey::ArrowDown),
                "home" => key = Some(ShortcutKey::Home),
                "end" => key = Some(ShortcutKey::End),
                "pageup" | "pgup" => key = Some(ShortcutKey::PageUp),
                "pagedown" | "pgdn" => key = Some(ShortcutKey::PageDown),
                _ => {
                    let mut chars = token.chars();
                    let ch = chars.next()?;
                    if chars.next().is_none() {
                        key = Some(ShortcutKey::Char(ch.to_ascii_uppercase()));
                    } else {
                        return None;
                    }
                }
            }
        }
        Some(Self {
            key: key?,
            command,
            shift,
            alt,
        })
    }

    pub fn matches(self, key: &Key, modifiers: Modifiers) -> bool {
        let command_matches = if self.command {
            platform::command_modifier_pressed(modifiers)
        } else {
            platform::command_modifier_released(modifiers)
        };
        command_matches
            && modifiers.shift == self.shift
            && modifiers.alt == self.alt
            && self.key.matches(key)
    }
}

impl ShortcutKey {
    fn matches(self, key: &Key) -> bool {
        match (self, key) {
            (Self::Char(expected), Key::Char(actual)) => {
                expected.eq_ignore_ascii_case(&actual.to_ascii_uppercase())
            }
            (Self::Insert, Key::Insert)
            | (Self::Delete, Key::Delete)
            | (Self::Backspace, Key::Backspace)
            | (Self::Enter, Key::Enter)
            | (Self::Escape, Key::Escape)
            | (Self::Tab, Key::Tab)
            | (Self::Space, Key::Char(' '))
            | (Self::ArrowLeft, Key::ArrowLeft)
            | (Self::ArrowRight, Key::ArrowRight)
            | (Self::ArrowUp, Key::ArrowUp)
            | (Self::ArrowDown, Key::ArrowDown)
            | (Self::Home, Key::Home)
            | (Self::End, Key::End)
            | (Self::PageUp, Key::PageUp)
            | (Self::PageDown, Key::PageDown) => true,
            _ => false,
        }
    }
}
