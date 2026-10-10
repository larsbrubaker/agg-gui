//! The add menu and the editor's keys beyond Delete: Shift+A, or a click
//! of the right button on empty canvas, opens the add menu at the pointer
//! (NodeDesigner's and Blender's gesture), and Escape closes it or goes to
//! the host (`NodeGraphModel::on_escape`).
//!
//! The add menu comes from one of three places, first match wins:
//! 1. a host widget from [`NodeEditor::with_add_menu`] (MatterCAD's search
//!    box over a list), placed with its top-left at the pointer and kept
//!    inside the editor, hosted like the floating row editors in
//!    `overlay_editors` (the host's `overlay_sink`, or the in-editor
//!    overlay, which a press outside it closes);
//! 2. host entries from `NodeGraphModel::add_menu`, shown in the editor's
//!    popup with each pick handed to `on_add_menu_action`;
//! 3. the built-in category submenus (`popup::build_add_node_popup_items`).
//!
//! Split out of `mod.rs` and `events.rs` for the 800-line guardrail; the
//! right-press that becomes a pan or a click lives in `events.rs`.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::{Event, EventResult, Key, Modifiers, Point, PopupMenu, Rect, Size, Widget};

use super::NodeEditor;

/// How far (logical px) the pointer may move between the right press and
/// its release for the gesture to still be a click rather than a pan.
/// NodeDesigner's `3 * DeviceScale` device pixels.
pub(crate) const CLICK_SLOP: f64 = 3.0;

/// What the editor tells [`NodeEditor::with_add_menu`]'s builder about
/// the place the menu opens.
#[derive(Clone, Debug)]
pub struct AddMenuRequest {
    /// The pointer in canvas space (Y up): where a picked node's card
    /// should put its top-left.
    pub canvas_pos: [f64; 2],
    /// The pointer in editor-local coordinates (Y up, origin bottom-left).
    pub local_pos: Point,
    /// The pointer in app-absolute logical coordinates (at worst one frame
    /// stale, see [`NodeEditor::app_to_canvas`]).
    pub app_pos: Point,
    /// The editor's size, for a host that sizes its menu to fit.
    pub editor_size: Size,
    /// Set it to `true` to close the menu (after a pick, or on Escape in
    /// the menu's own search box); the editor drains it on its next event
    /// or layout pass, as for its own floating editors.
    pub close: Rc<Cell<bool>>,
}

/// Builds the host's add-menu widget, or `None` to open nothing.
pub(crate) type AddMenuBuilder = Box<dyn FnMut(AddMenuRequest) -> Option<Box<dyn Widget>>>;

/// The add menu's state on [`NodeEditor`].
#[derive(Default)]
pub(crate) struct AddMenuState {
    /// The host's widget builder ([`NodeEditor::with_add_menu`]).
    pub builder: Option<AddMenuBuilder>,
    /// The close flag of the host widget open in the in-editor overlay.
    pub close: Option<Rc<Cell<bool>>>,
    /// The popup holds the add menu (not a node's context menu).
    pub in_popup: bool,
    /// The popup holds `NodeGraphModel::add_menu`'s entries.
    pub host_popup: bool,
    /// A right-drag on empty canvas pans and a right-click opens the add
    /// menu on release ([`NodeEditor::with_right_drag_pan`]).
    pub right_drag_pan: bool,
    /// The last pointer position seen, editor-local: where Shift+A opens.
    pub last_pointer: Option<Point>,
}

impl NodeEditor {
    /// Install the host's add menu: `builder` gets an [`AddMenuRequest`]
    /// whenever Shift+A or a right-click on empty canvas asks for the menu
    /// and returns the widget to show (or `None`). A widget that comes back
    /// with empty bounds is laid out and placed with its top-left at the
    /// pointer, kept inside the editor; non-empty bounds (editor-local) are
    /// kept. With an `overlay_sink` the widget goes to the sink in
    /// app-absolute bounds and the host closes it; without one the editor
    /// shows it over the canvas, routes events to it first, and closes it on
    /// a press outside it or on Escape. Takes precedence over
    /// `NodeGraphModel::add_menu`.
    pub fn with_add_menu<F>(mut self, builder: F) -> Self
    where
        F: FnMut(AddMenuRequest) -> Option<Box<dyn Widget>> + 'static,
    {
        self.add_menu.builder = Some(Box::new(builder));
        self
    }

    /// With `true`, a right-drag on empty canvas pans and a right-click
    /// (released within 3 px of the press) opens the add menu at the
    /// release, as NodeDesigner does. Off by default: the right press opens
    /// the add menu at once and right-drags do nothing.
    pub fn with_right_drag_pan(mut self, enabled: bool) -> Self {
        self.add_menu.right_drag_pan = enabled;
        self
    }

    /// Open the add menu at `local` (editor-local), as Shift+A and a
    /// right-click on empty canvas do.
    pub fn open_add_menu(&mut self, local: Point) {
        self.close_add_menu();
        let canvas_pos = self.local_to_canvas(local);
        self.popup_canvas_pos = canvas_pos;
        self.popup_host_node = None;
        if self.add_menu.builder.is_some() {
            self.open_host_add_menu(local, canvas_pos);
            return;
        }
        if self.rebuild_popup_for_empty_canvas() {
            self.popup.open_at(local);
        }
        // Opening a menu must invalidate or it won't paint until the next
        // unrelated event triggers a redraw.
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
    }

    /// Whether the add menu is open in the editor (the popup showing it, or
    /// the host widget in the in-editor overlay).
    pub fn is_add_menu_open(&self) -> bool {
        self.add_menu_overlay_open() || (self.popup.is_open() && self.add_menu.in_popup)
    }

    /// Close the add menu if it is open in the editor.
    pub fn close_add_menu(&mut self) {
        if self.add_menu_overlay_open() {
            self.close_overlay();
        }
        self.add_menu.close = None;
        if self.popup.is_open() && self.add_menu.in_popup {
            self.popup.close();
            self.backbuffer.invalidate();
            agg_gui::animation::request_draw();
        }
    }

    fn open_host_add_menu(&mut self, local: Point, canvas_pos: [f64; 2]) {
        let (ox, oy) = self.last_abs_origin.get();
        let size = Size::new(self.bounds.width, self.bounds.height);
        let close = Rc::new(Cell::new(false));
        let request = AddMenuRequest {
            canvas_pos,
            local_pos: local,
            app_pos: Point::new(local.x + ox, local.y + oy),
            editor_size: size,
            close: Rc::clone(&close),
        };
        let Some(builder) = self.add_menu.builder.as_mut() else {
            return;
        };
        let Some(mut menu) = builder(request) else {
            return;
        };
        let mut b = menu.bounds();
        if b.width <= 0.0 || b.height <= 0.0 {
            let desired = menu.layout(size);
            b = add_menu_rect(local, desired, size);
        }
        if let Some(sink) = self.overlay_sink.as_mut() {
            menu.set_bounds(Rect::new(b.x + ox, b.y + oy, b.width, b.height));
            sink(menu, close);
        } else {
            menu.set_bounds(b);
            self.overlay = Some(menu);
            self.overlay_close_flag = Some(Rc::clone(&close));
            self.add_menu.close = Some(close);
        }
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
    }

    /// True while the host's add-menu widget is the in-editor overlay.
    pub(super) fn add_menu_overlay_open(&self) -> bool {
        match (&self.add_menu.close, &self.overlay_close_flag) {
            (Some(a), Some(b)) => self.overlay.is_some() && Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Run first in `on_event`: remembers the pointer for Shift+A, and a
    /// press outside an open in-editor add menu closes it (the press then
    /// goes on to whatever is under it, as NodeDesigner's `TakesPress`).
    pub(super) fn add_menu_preview(&mut self, event: &Event) {
        match event {
            Event::MouseMove { pos } => self.add_menu.last_pointer = Some(*pos),
            Event::MouseDown { pos, .. } => {
                self.add_menu.last_pointer = Some(*pos);
                if self.add_menu_overlay_open() {
                    let inside = self.overlay.as_ref().is_some_and(|o| {
                        let b = o.bounds();
                        pos.x >= b.x
                            && pos.x <= b.x + b.width
                            && pos.y >= b.y
                            && pos.y <= b.y + b.height
                    });
                    if !inside {
                        self.close_add_menu();
                    }
                }
            }
            _ => {}
        }
    }

    /// Shift+A and Escape, ahead of the Delete handling in `on_key_down`.
    /// `None` leaves the key to it.
    pub(super) fn add_menu_key_down(&mut self, key: &Key, mods: Modifiers) -> Option<EventResult> {
        match key {
            Key::Char('a' | 'A') if mods.shift && !mods.ctrl && !mods.alt && !mods.meta => {
                if self.is_add_menu_open() {
                    return Some(EventResult::Consumed);
                }
                let at = self.add_menu.last_pointer.unwrap_or(Point::new(
                    self.bounds.width * 0.5,
                    self.bounds.height * 0.5,
                ));
                self.open_add_menu(at);
                Some(EventResult::Consumed)
            }
            Key::Escape => {
                if self.add_menu_overlay_open() || self.popup.is_open() {
                    self.close_add_menu();
                    if self.popup.is_open() {
                        self.popup.close();
                        agg_gui::animation::request_draw();
                    }
                    return Some(EventResult::Consumed);
                }
                let handled = self.model.lock().unwrap().on_escape();
                Some(if handled {
                    agg_gui::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                })
            }
            _ => None,
        }
    }

    /// Fill the popup with the add menu: the host's entries
    /// (`NodeGraphModel::add_menu` at [`Self::popup_canvas_pos`]) or the
    /// built-in submenus. Returns `false` when it has no entries.
    pub(super) fn rebuild_popup_for_empty_canvas(&mut self) -> bool {
        let host = self.model.lock().unwrap().add_menu(self.popup_canvas_pos);
        self.add_menu.host_popup = host.is_some();
        self.add_menu.in_popup = true;
        let items = host.unwrap_or_else(|| super::build_add_node_popup_items(&self.model));
        let open = !items.is_empty();
        self.popup = PopupMenu::new(items);
        self.popup_host_node = None;
        open
    }
}

/// The add menu's rect for a `size` menu opened at `pointer` in an
/// `editor`-sized pane: top-left at the pointer, kept inside the editor
/// (NodeDesigner's `NodeEditorAddMenu.Open`; both are Y up).
pub(crate) fn add_menu_rect(pointer: Point, size: Size, editor: Size) -> Rect {
    let left = pointer.x.min(editor.width - size.width).max(0.0);
    let bottom = (pointer.y - size.height)
        .min(editor.height - size.height)
        .max(0.0);
    Rect::new(left, bottom, size.width, size.height)
}
