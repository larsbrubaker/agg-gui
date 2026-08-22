//! ModalSheet — a centered modal panel over a dimming scrim, the
//! SwiftUI-`.sheet` / desktop-dialog presentation.
//!
//! Unlike [`Window`](super::window::Window) the sheet is not draggable,
//! resizable, or collapsible: it is a fixed-size surface centered over
//! the app, with a translucent scrim blocking interaction with
//! everything behind it. Place it as a late child of the root
//! [`Stack`](super::primitives::Stack) so it paints above the app; while
//! its visibility cell is `true`, `has_active_modal` routes all input to
//! the sheet subtree (the scrim swallows what the panel doesn't use) and
//! `Escape` closes it.
//!
//! # Default / cancel actions
//!
//! Return / Enter that reaches the sheet unconsumed (focus is not on a
//! text input — those consume Enter) activates the sheet's **default
//! action**: the first visible content widget whose `WidgetBase` has
//! `default_action` set (a `Button::with_default_action()`), or the
//! closure from [`with_default_action`](ModalSheet::with_default_action).
//! Escape likewise runs the **cancel action** — a
//! `Button::with_cancel_action()` in the content or a
//! [`with_cancel_action`](ModalSheet::with_cancel_action) closure — and
//! only falls back to the plain close when neither is present. Because
//! keys are routed to the topmost active modal only, a stacked sheet's
//! default button never fires the one beneath it.
//!
//! A showing sheet OWNS Enter / Escape for its whole scope: while any
//! modal is active the [`App`](crate::App) skips the root-level
//! default / cancel dispatch entirely, so a sheet without a default
//! action still shadows the buttons behind it instead of firing them
//! under the scrim. That applies to
//! [`with_key_passthrough`](ModalSheet::with_key_passthrough) sheets too
//! — passthrough forwards ordinary keys to the app behind, never the
//! sheet's own Enter / Escape semantics.
//!
//! ```ignore
//! let visible = Rc::new(Cell::new(false));
//! let sheet = ModalSheet::new(Rc::clone(&visible), Box::new(content))
//!     .with_panel_size(Size::new(620.0, 420.0));
//! Stack::new().add(Box::new(app)).add(Box::new(sheet))
//! ```

use std::cell::Cell;
use std::rc::Rc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key};
use crate::geometry::{Rect, Size};
use crate::layout_props::WidgetBase;
use crate::theme::current_visuals;
use crate::widget::{activate_action_at, cancel_action_path, default_action_path, Widget};
use crate::widgets::window::chrome::{paint_chrome_shadow, ChromeStyle};

/// Minimum gap kept between the panel and the host bounds when the
/// desired panel size doesn't fit.
const EDGE_MARGIN: f64 = 24.0;
const CORNER_R: f64 = 10.0;

/// A centered, fixed-size modal panel over a scrim. One content child.
pub struct ModalSheet {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // exactly 1: the content
    base: WidgetBase,
    visible: Rc<Cell<bool>>,
    /// Desired panel size; clamped to the available bounds at layout.
    panel_size: Size,
    /// Panel rect in local coordinates, computed each layout.
    panel: Rect,
    /// Dismiss on `Escape` (default true; SwiftUI's `.cancelAction`).
    escape_closes: bool,
    /// Let unhandled keys fall through to the app behind the sheet
    /// (default false). For sheets that measure live input — a latency
    /// calibration tapping along on the app's instrument keys.
    key_passthrough: bool,
    /// Invoked whenever the sheet closes itself (Escape). External
    /// closes (a Done button clearing the cell) don't route through
    /// this — wire those callbacks at the button.
    on_close: Option<Box<dyn Fn()>>,
    /// Closure-form default action (Enter); see the module docs. A
    /// `Button::with_default_action()` in the content takes precedence.
    default_action: Option<Box<dyn Fn()>>,
    /// Closure-form cancel action (Escape). When set, Escape runs it
    /// INSTEAD of closing the sheet — the action owns dismissal, exactly
    /// like the Cancel button it mirrors.
    cancel_action: Option<Box<dyn Fn()>>,
}

impl ModalSheet {
    pub fn new(visible: Rc<Cell<bool>>, content: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![content],
            base: WidgetBase::new(),
            visible,
            panel_size: Size::new(480.0, 360.0),
            panel: Rect::default(),
            escape_closes: true,
            key_passthrough: false,
            on_close: None,
            default_action: None,
            cancel_action: None,
        }
    }

    /// Let unhandled keys reach the app behind the sheet (see
    /// `key_passthrough`). Enter / Escape stay the sheet's own: the
    /// root-level default / cancel action is suppressed while any modal
    /// is showing (see the module docs).
    pub fn with_key_passthrough(mut self, passthrough: bool) -> Self {
        self.key_passthrough = passthrough;
        self
    }

    /// Desired panel size (clamped to the host bounds minus a margin).
    pub fn with_panel_size(mut self, size: Size) -> Self {
        self.panel_size = size;
        self
    }

    /// Disable the default Escape-to-close behaviour.
    pub fn with_escape_closes(mut self, close: bool) -> Self {
        self.escape_closes = close;
        self
    }

    /// Run a callback when the sheet dismisses itself via Escape.
    pub fn with_on_close(mut self, on_close: impl Fn() + 'static) -> Self {
        self.on_close = Some(Box::new(on_close));
        self
    }

    /// Run `action` when Return / Enter reaches the sheet unconsumed and
    /// no content widget is marked `with_default_action()`. Mirrors
    /// SwiftUI's `.keyboardShortcut(.defaultAction)` on the sheet's
    /// primary button; the closure owns whatever dismissal it wants.
    pub fn with_default_action(mut self, action: impl Fn() + 'static) -> Self {
        self.default_action = Some(Box::new(action));
        self
    }

    /// Run `action` on Escape instead of the built-in close. The closure
    /// owns dismissal (typically it clears the visibility cell, exactly
    /// as the Cancel button's `on_click` does). A content widget marked
    /// `with_cancel_action()` is tried first; `with_on_close` is NOT
    /// invoked on this path because the sheet did not close itself.
    pub fn with_cancel_action(mut self, action: impl Fn() + 'static) -> Self {
        self.cancel_action = Some(Box::new(action));
        self
    }

    pub fn visibility_cell(&self) -> Rc<Cell<bool>> {
        Rc::clone(&self.visible)
    }

    fn close(&self) {
        self.visible.set(false);
        if let Some(on_close) = &self.on_close {
            on_close();
        }
        crate::animation::request_draw();
    }

    /// Activate the content widget at `path` (found by one of the
    /// `*_action_path` searches). Dispatches into the content child only,
    /// never back to the sheet, so a declining target can't re-enter
    /// this handler.
    fn activate_content(&mut self, path: &[usize]) -> bool {
        let Some((&first, rest)) = path.split_first() else {
            return false;
        };
        let Some(child) = self.children.get_mut(first) else {
            return false;
        };
        activate_action_at(child.as_mut(), rest).is_consumed()
    }

    /// Enter reached the sheet: fire the default action if there is one.
    fn handle_enter(&mut self) -> bool {
        if let Some(path) = default_action_path(self, false) {
            if self.activate_content(&path) {
                return true;
            }
        }
        if let Some(action) = &self.default_action {
            action();
            crate::animation::request_draw();
            return true;
        }
        false
    }

    /// Escape reached the sheet: cancel-action widget, then closure, then
    /// the plain close.
    fn handle_escape(&mut self) -> bool {
        if let Some(path) = cancel_action_path(self, false) {
            if self.activate_content(&path) {
                return true;
            }
        }
        if let Some(action) = &self.cancel_action {
            action();
            crate::animation::request_draw();
            return true;
        }
        if self.escape_closes {
            self.close();
            return true;
        }
        false
    }
}

impl Widget for ModalSheet {
    fn type_name(&self) -> &'static str {
        "ModalSheet"
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }

    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }

    fn is_visible(&self) -> bool {
        self.visible.get()
    }

    fn has_active_modal(&self) -> bool {
        self.visible.get()
    }

    fn layout(&mut self, available: Size) -> Size {
        if !self.visible.get() {
            self.bounds = Rect::new(0.0, 0.0, 0.0, 0.0);
            return Size::new(0.0, 0.0);
        }
        let w = self
            .panel_size
            .width
            .min(available.width - 2.0 * EDGE_MARGIN)
            .max(0.0);
        let h = self
            .panel_size
            .height
            .min(available.height - 2.0 * EDGE_MARGIN)
            .max(0.0);
        // Centered, snapped to whole pixels for crisp chrome.
        let x = ((available.width - w) / 2.0).round();
        let y = ((available.height - h) / 2.0).round();
        self.panel = Rect::new(x, y, w, h);
        if let Some(child) = self.children.first_mut() {
            child.layout(Size::new(w, h));
            child.set_bounds(self.panel);
        }
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.visible.get() {
            return;
        }
        let v = current_visuals();

        // Scrim: dim everything behind the sheet.
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.20));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();

        // Panel chrome: shadow + rounded body + hairline border.
        let style = ChromeStyle {
            corner_radius: CORNER_R,
            ..ChromeStyle::from_visuals(&v)
        };
        ctx.save();
        ctx.translate(self.panel.x, self.panel.y);
        paint_chrome_shadow(ctx, self.panel.width, self.panel.height, &style);
        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, self.panel.width, self.panel.height, CORNER_R);
        ctx.fill();
        ctx.set_stroke_color(v.window_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(
            0.5,
            0.5,
            self.panel.width - 1.0,
            self.panel.height - 1.0,
            CORNER_R,
        );
        ctx.stroke();
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.visible.get() {
            return EventResult::Ignored;
        }
        match event {
            // Scrim: swallow pointer input the panel content didn't take,
            // so nothing behind the sheet reacts (no click-outside close —
            // matching macOS sheets).
            Event::MouseDown { .. } | Event::MouseUp { .. } | Event::MouseWheel { .. } => {
                EventResult::Consumed
            }
            Event::KeyDown {
                key: Key::Escape, ..
            } if self.handle_escape() => EventResult::Consumed,
            Event::KeyDown {
                key: Key::Enter, ..
            } if self.handle_enter() => EventResult::Consumed,
            // Swallow every other key the panel content didn't take:
            // keystrokes must not fall through to shortcut handlers
            // behind the sheet (a piano app's key-to-note routing, say).
            Event::KeyDown { .. } | Event::KeyUp { .. } if !self.key_passthrough => {
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}
