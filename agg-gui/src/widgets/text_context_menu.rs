//! Shared right-click Cut / Copy / Paste / Select-All context menu for the
//! text-editing widgets ([`TextField`](crate::widgets::TextField),
//! [`TextArea`](crate::widgets::TextArea) and
//! [`RichTextEdit`](crate::widgets::RichTextEdit)).
//!
//! Each widget embeds a [`TextContextMenu`], opens it on right-click with its
//! current selection / read-only state, routes events into it while it is open,
//! and dispatches the chosen [`TextMenuAction`] back through its OWN existing
//! keyboard-shortcut handlers. No clipboard logic lives here — this module only
//! owns the menu presentation and event plumbing, reusing the same
//! [`PopupMenu`] machinery as the menu bar and the popups demo so the styling,
//! nesting, hover, Escape handling and viewport clamping stay identical.

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Modifiers, MouseButton};
use crate::geometry::Point;
use crate::text::Font;
use crate::widget::current_viewport;
use crate::widgets::menu::{MenuEntry, MenuItem, MenuResponse, PopupMenu};

/// The four standard editing commands the text context menu offers. The caller
/// maps each back onto its own copy/cut/paste/select-all handler so there is a
/// single implementation shared with the keyboard shortcuts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextMenuAction {
    Cut,
    Copy,
    Paste,
    SelectAll,
}

impl TextMenuAction {
    fn id(self) -> &'static str {
        match self {
            TextMenuAction::Cut => "cut",
            TextMenuAction::Copy => "copy",
            TextMenuAction::Paste => "paste",
            TextMenuAction::SelectAll => "select-all",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        match id {
            "cut" => Some(Self::Cut),
            "copy" => Some(Self::Copy),
            "paste" => Some(Self::Paste),
            "select-all" => Some(Self::SelectAll),
            _ => None,
        }
    }
}

/// Embeddable controller wrapping an optional open [`PopupMenu`]. `Default`
/// yields a closed menu, so a widget can hold one with `..Default::default()`.
#[derive(Clone, Default)]
pub struct TextContextMenu {
    menu: Option<PopupMenu>,
}

impl TextContextMenu {
    pub fn new() -> Self {
        Self { menu: None }
    }

    pub fn is_open(&self) -> bool {
        self.menu.as_ref().map(PopupMenu::is_open).unwrap_or(false)
    }

    pub fn close(&mut self) {
        self.menu = None;
    }

    /// Build the items honouring enablement, then open at `pos` (the right-click
    /// position, in the widget's local coordinates — the same space its
    /// `paint_global_overlay` paints in).
    ///
    /// * `has_selection` — Cut and Copy are enabled only with a selection.
    /// * `editable` — Cut and Paste are enabled only when the widget accepts
    ///   input (not read-only / disabled). Copy stays available read-only, and
    ///   Select All is always enabled.
    pub fn open(&mut self, pos: Point, has_selection: bool, editable: bool) {
        let mut cut = MenuItem::action("Cut", TextMenuAction::Cut.id())
            .icon('\u{F0C4}') // FA scissors
            .shortcut("Ctrl+X");
        if !(has_selection && editable) {
            cut = cut.disabled();
        }
        let mut copy = MenuItem::action("Copy", TextMenuAction::Copy.id())
            .icon('\u{F0C5}') // FA copy
            .shortcut("Ctrl+C");
        if !has_selection {
            copy = copy.disabled();
        }
        let mut paste = MenuItem::action("Paste", TextMenuAction::Paste.id())
            .icon('\u{F0EA}') // FA paste
            .shortcut("Ctrl+V");
        if !editable {
            paste = paste.disabled();
        }
        let select_all =
            MenuItem::action("Select All", TextMenuAction::SelectAll.id()).shortcut("Ctrl+A");

        let items: Vec<MenuEntry> = vec![
            cut.into(),
            copy.into(),
            paste.into(),
            MenuEntry::Separator,
            select_all.into(),
        ];
        let mut menu = PopupMenu::new(items);
        menu.open_at(pos);
        self.menu = Some(menu);
    }

    /// Route an event into the open menu. Returns the menu's [`EventResult`]
    /// (consume it if `is_consumed()`) and any chosen action for the caller to
    /// dispatch. Choosing an item or dismissing the menu closes it here.
    pub fn handle_event(&mut self, event: &Event) -> (EventResult, Option<TextMenuAction>) {
        let Some(menu) = self.menu.as_mut() else {
            return (EventResult::Ignored, None);
        };
        let (result, response) = menu.handle_event(event, current_viewport());
        let action = match response {
            MenuResponse::Action(id) => {
                self.menu = None;
                TextMenuAction::from_id(&id)
            }
            MenuResponse::Closed => {
                self.menu = None;
                None
            }
            MenuResponse::None => None,
        };
        (result, action)
    }

    /// Convenience wrapper for a synthetic left click at `pos` — used by tests
    /// to activate a menu item through the real menu event path.
    pub fn click_at(&mut self, pos: Point) -> Option<TextMenuAction> {
        let event = Event::MouseDown {
            pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        };
        self.handle_event(&event).1
    }

    pub fn paint(&mut self, ctx: &mut dyn DrawCtx, font: Arc<Font>, font_size: f64) {
        if let Some(menu) = self.menu.as_mut() {
            menu.paint(ctx, font, font_size, current_viewport());
        }
    }

    /// Test-only: report whether the given action's item is enabled in the
    /// currently-open menu (`None` when closed or the item is absent).
    #[cfg(test)]
    pub(crate) fn action_enabled(&self, action: TextMenuAction) -> Option<bool> {
        let menu = self.menu.as_ref()?;
        for entry in &menu.items {
            if let MenuEntry::Item(item) = entry {
                if item.action.as_deref() == Some(action.id()) {
                    return Some(item.enabled);
                }
            }
        }
        None
    }

    /// Test-only: the center point of an action's laid-out row, so a test can
    /// dispatch a real click there (through the widget or the menu directly).
    #[cfg(test)]
    pub(crate) fn action_row_center(&self, action: TextMenuAction) -> Option<Point> {
        let menu = self.menu.as_ref()?;
        let layouts = menu.state.layouts(&menu.items, current_viewport());
        let panel = layouts.first()?;
        for row in &panel.rows {
            let Some(idx) = row.item_index else { continue };
            if let Some(MenuEntry::Item(item)) = menu.items.get(idx) {
                if item.action.as_deref() == Some(action.id()) {
                    return Some(Point::new(
                        row.rect.x + row.rect.width * 0.5,
                        row.rect.y + row.rect.height * 0.5,
                    ));
                }
            }
        }
        None
    }

    /// Test-only: activate an action by a real click on its laid-out row,
    /// exercising the menu's hit-testing / activation path end to end.
    #[cfg(test)]
    pub(crate) fn click_action(&mut self, action: TextMenuAction) -> Option<TextMenuAction> {
        let center = self.action_row_center(action)?;
        self.click_at(center)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;

    /// Item enablement must reflect the selection and read-only state exactly:
    /// Cut/Copy need a selection, Cut/Paste need an editable target, Select All
    /// is always available.
    #[test]
    fn item_enablement_tracks_selection_and_editability() {
        let at = Point::new(10.0, 10.0);

        // Editable, with a selection: everything enabled.
        let mut m = TextContextMenu::new();
        m.open(at, /*has_selection=*/ true, /*editable=*/ true);
        assert_eq!(m.action_enabled(TextMenuAction::Cut), Some(true));
        assert_eq!(m.action_enabled(TextMenuAction::Copy), Some(true));
        assert_eq!(m.action_enabled(TextMenuAction::Paste), Some(true));
        assert_eq!(m.action_enabled(TextMenuAction::SelectAll), Some(true));

        // Editable, no selection: Cut/Copy disabled, Paste/Select All enabled.
        let mut m = TextContextMenu::new();
        m.open(at, false, true);
        assert_eq!(m.action_enabled(TextMenuAction::Cut), Some(false));
        assert_eq!(m.action_enabled(TextMenuAction::Copy), Some(false));
        assert_eq!(m.action_enabled(TextMenuAction::Paste), Some(true));
        assert_eq!(m.action_enabled(TextMenuAction::SelectAll), Some(true));

        // Read-only, with a selection: Copy stays, Cut/Paste disabled.
        let mut m = TextContextMenu::new();
        m.open(at, true, false);
        assert_eq!(m.action_enabled(TextMenuAction::Cut), Some(false));
        assert_eq!(m.action_enabled(TextMenuAction::Copy), Some(true));
        assert_eq!(m.action_enabled(TextMenuAction::Paste), Some(false));
        assert_eq!(m.action_enabled(TextMenuAction::SelectAll), Some(true));
    }

    /// A real click on the Copy row returns the Copy action through the menu's
    /// own hit-testing, and closes the menu.
    #[test]
    fn clicking_copy_row_activates_copy_and_closes() {
        crate::widget::set_current_viewport(crate::geometry::Size::new(800.0, 600.0));
        let mut m = TextContextMenu::new();
        m.open(Point::new(10.0, 10.0), true, true);
        assert!(m.is_open());
        assert_eq!(
            m.click_action(TextMenuAction::Copy),
            Some(TextMenuAction::Copy)
        );
        assert!(!m.is_open(), "activating an item closes the menu");
    }
}
