//! Data model for popup and menu-bar menus.
//!
//! Menus are application-owned item trees. The widget layer interprets this
//! model for painting, hit testing, keyboard navigation, and action dispatch.

use crate::event::{Key, Modifiers};
use crate::platform;

#[derive(Clone, Debug)]
pub enum MenuEntry {
    Item(MenuItem),
    Separator,
}

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub label: String,
    pub icon: Option<char>,
    pub shortcut: Option<String>,
    pub accelerator: Option<MenuShortcut>,
    pub enabled: bool,
    pub selection: MenuSelection,
    pub action: Option<String>,
    pub submenu: Vec<MenuEntry>,
    pub close_on_activate: bool,
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
            shortcut: None,
            accelerator: None,
            enabled: true,
            selection: MenuSelection::None,
            action: Some(action.into()),
            submenu: Vec::new(),
            close_on_activate: true,
        }
    }

    pub fn submenu(label: impl Into<String>, submenu: Vec<MenuEntry>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            shortcut: None,
            accelerator: None,
            enabled: true,
            selection: MenuSelection::None,
            action: None,
            submenu,
            close_on_activate: false,
        }
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

    pub fn display_text(self) -> String {
        let mut parts = Vec::new();
        if self.command {
            parts.push(platform::primary_modifier_label().to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        parts.push(self.key.display_text());
        parts.join("+")
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
            | (Self::Escape, Key::Escape) => true,
            _ => false,
        }
    }

    fn display_text(self) -> String {
        match self {
            Self::Char(ch) => ch.to_string(),
            Self::Insert => "Insert".to_string(),
            Self::Delete => "Delete".to_string(),
            Self::Backspace => "Backspace".to_string(),
            Self::Enter => "Enter".to_string(),
            Self::Escape => "Esc".to_string(),
        }
    }
}
