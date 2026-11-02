//! The menu button a hovered row or tile draws in its top-right corner (the overflow glyph):
//! where it is drawn, the square a click on it must land in, and the press state that has to
//! survive the list being rebuilt under the pointer ([`MenuButtonState`], kept in
//! `BrowserState::menu_button`). Both item views use it (`list/row.rs`, `list/icon_view.rs`, their
//! presses through `list/item_event.rs`), and the list (`list/mod.rs`) opens the item menu
//! hanging from the button.
//!
//! The clickable square is centred on the glyph: 24 units across with a mouse, 44 (a finger) on a
//! touch screen. An item too narrow to hold the glyph twice over (the smallest tiles) draws no
//! button and has no clickable square; its menu is the right click's. A press counts as a press
//! on the button when it lands in that square and is a mouse's: a mouse comes over the item
//! (bringing the button) before it presses, but a finger's touch brings the hover with it, so a
//! tap on that corner is a tap on the item; a finger opens the menu with a long press, the right
//! click. Whether the glyph has been painted yet plays no part: on a slow frame the click must
//! still open the menu.

use agg_gui::{Point, Rect};

/// The overflow glyph's side, in logical units.
pub const MENU_GLYPH_SIZE: f64 = 32.0;
/// How far below the item's top the glyph's box starts.
const MENU_GLYPH_TOP_GAP: f64 = 3.0;
/// The clickable square's side with a mouse.
pub const MENU_BUTTON_MOUSE: f64 = 24.0;
/// The clickable square's side on a touch screen: a finger's target.
pub const MENU_BUTTON_TOUCH: f64 = 44.0;

/// Whether an item `width` wide draws the menu button: it needs room for the glyph twice over, so
/// the button never covers a small tile whole.
pub fn shows_menu_button(width: f64) -> bool {
    width >= 2.0 * MENU_GLYPH_SIZE
}

/// The glyph's box (item-local, Y up) in an item `width` by `height`.
pub fn menu_glyph_rect(width: f64, height: f64) -> Rect {
    Rect::new(
        width - MENU_GLYPH_SIZE,
        height - MENU_GLYPH_SIZE - MENU_GLYPH_TOP_GAP,
        MENU_GLYPH_SIZE,
        MENU_GLYPH_SIZE,
    )
}

/// The clickable square's side here: a finger's while agg-gui's touch UI is active, else a
/// mouse's.
pub fn menu_button_side() -> f64 {
    if agg_gui::input_profile::touch_ui_active() {
        MENU_BUTTON_TOUCH
    } else {
        MENU_BUTTON_MOUSE
    }
}

/// The clickable square (item-local, Y up), centred on the glyph; `None` for an item that draws
/// no button.
pub fn menu_button_rect(width: f64, height: f64) -> Option<Rect> {
    if !shows_menu_button(width) {
        return None;
    }
    let r = menu_glyph_rect(width, height);
    let (cx, cy) = (r.x + r.width / 2.0, r.y + r.height / 2.0);
    let side = menu_button_side();
    Some(Rect::new(cx - side / 2.0, cy - side / 2.0, side, side))
}

/// Whether `pos` (item-local) is on the menu button of an item `width` by `height` (edges
/// included).
pub fn in_menu_button(pos: Point, width: f64, height: f64) -> bool {
    menu_button_rect(width, height).is_some_and(|r| {
        pos.x >= r.x && pos.x <= r.x + r.width && pos.y >= r.y && pos.y <= r.y + r.height
    })
}

/// The list's menu-button state, kept in the browser's state so it survives the list being
/// rebuilt under the pointer (a press selects, and a selection rebuilds).
#[derive(Debug, Default)]
pub struct MenuButtonState {
    /// The row the pointer is over, so a row rebuilt under a still pointer still shows its button.
    hovered: Option<usize>,
    /// The row whose button the current press landed on (a mouse's press).
    pressed: Option<usize>,
    /// Where the item menu a button click asked for hangs from (window coordinates, Y up): the
    /// button's bottom-left corner.
    anchor: Option<Point>,
}

impl MenuButtonState {
    /// The pointer came over row `index` (`true`) or left it.
    pub fn hover(&mut self, index: usize, inside: bool) {
        crate::trace(|| format!("tile hover #{index} {}", if inside { "in" } else { "out" }));
        if inside {
            self.hovered = Some(index);
        } else if self.hovered == Some(index) {
            self.hovered = None;
        }
    }

    /// Whether the pointer is over row `index`.
    pub fn is_hovered(&self, index: usize) -> bool {
        self.hovered == Some(index)
    }

    /// A left press on row `index`: whether it landed on the button (`on_button`) with a mouse.
    pub fn press(&mut self, index: usize, on_button: bool) {
        let mouse = !agg_gui::last_press_was_touch();
        self.pressed = (on_button && mouse).then_some(index);
        crate::trace(|| format!("menu_button press #{index} on_button={on_button} mouse={mouse}"));
    }

    /// The release of the press on row `index`: whether it is a click on the button (the press
    /// was, and the release is on it too). Clears the press either way.
    pub fn release(&mut self, index: usize, on_button: bool) -> bool {
        let pressed = self.pressed.take();
        let click = pressed == Some(index) && on_button;
        crate::trace(|| format!("menu_button release #{index} on_button={on_button} -> {click}"));
        click
    }

    /// The menu a button click asks for hangs from `anchor` (window coordinates, Y up).
    pub fn set_anchor(&mut self, anchor: Point) {
        self.anchor = Some(anchor);
    }

    /// Where the requested item menu hangs from, when a button asked for it; clears it.
    pub fn take_anchor(&mut self) -> Option<Point> {
        self.anchor.take()
    }
}
