// Extra title-bar buttons for `Window` — a port of agg-sharp's
// `WindowWidget.AddTitleBarButton`, plus `Maximizable`, which decides
// whether the maximize button they sit beside is shown.
//
// The buttons are children of the `WindowTitleBar` sub-widget (which places
// them left of the maximize / close buttons; see `window_title_bar.rs`). The
// title bar is not in `Window.children` (the content owns that slot), so the
// App's hit-test stops at the window and the window forwards pointer events
// itself: `events.rs` hands the press to the title-bar subtree as it always
// has, and calls the helpers here to deliver hover moves and the release to
// the buttons. The window answers `tooltip_text` with the hovered button's
// tip so the App's tooltip pass shows it.

use super::*;
use crate::widget::dispatch_event_dyn;
use crate::widgets::window_title_bar::FIRST_EXTRA;

impl Window {
    /// Put `button` in the title bar, left of the maximize and close buttons.
    /// Each later button sits nearer the close button, as agg-sharp inserts
    /// each one just before its close button; the close and maximize buttons
    /// themselves never move.
    pub fn add_title_bar_button(&mut self, button: Box<dyn Widget>) {
        self.title_bar.add_button(button);
        self.backbuffer.invalidate();
        crate::animation::request_draw();
    }

    /// Builder form of [`Window::add_title_bar_button`].
    pub fn with_title_bar_button(mut self, button: Box<dyn Widget>) -> Self {
        self.add_title_bar_button(button);
        self
    }

    /// Whether the maximize button and double-click-to-maximize are offered.
    pub fn maximizable(&self) -> bool {
        self.maximizable
    }

    /// Offer or hide the maximize button and double-click-to-maximize
    /// (agg-sharp `WindowWidget.Maximizable`). Hidden, the close button
    /// stays put and added title-bar buttons sit directly left of it.
    /// Programmatic maximize state is left alone.
    pub fn set_maximizable(&mut self, maximizable: bool) {
        if self.maximizable != maximizable {
            self.maximizable = maximizable;
            self.maximize_hovered = false;
            self.title_bar.set_maximizable(maximizable);
            self.backbuffer.invalidate();
            crate::animation::request_draw();
        }
    }

    /// Builder form of [`Window::set_maximizable`].
    pub fn with_maximizable(mut self, maximizable: bool) -> Self {
        self.set_maximizable(maximizable);
        self
    }

    /// The buttons added with [`Window::add_title_bar_button`], in order.
    pub fn title_bar_buttons(&self) -> &[Box<dyn Widget>] {
        self.title_bar.buttons()
    }

    /// Mutable access to the added title-bar buttons, in order.
    pub fn title_bar_buttons_mut(&mut self) -> &mut [Box<dyn Widget>] {
        self.backbuffer.invalidate();
        self.title_bar.buttons_mut()
    }

    /// Bounds of added title-bar button `index` in window-local (Y-up)
    /// coordinates, as of the last layout.
    pub fn title_bar_button_rect(&self, index: usize) -> Option<Rect> {
        let tb = self.title_bar.bounds();
        self.title_bar.buttons().get(index).map(|b| {
            let r = b.bounds();
            Rect::new(tb.x + r.x, tb.y + r.y, r.width, r.height)
        })
    }

    fn title_bar_local(&self, pos: Point) -> Point {
        let tb = self.title_bar.bounds();
        Point::new(pos.x - tb.x, pos.y - tb.y)
    }

    /// Deliver a pointer move to every added button (each sees its own local
    /// position, so one the pointer left drops its hover) and record which
    /// one is under the pointer for [`Window::hovered_title_button_tip`].
    pub(super) fn forward_title_button_move(&mut self, pos: Point) {
        if !self.chrome {
            return;
        }
        let tb_local = self.title_bar_local(pos);
        let event = Event::MouseMove { pos: tb_local };
        let mut hovered = None;
        for i in 0..self.title_bar.buttons().len() {
            let idx = FIRST_EXTRA + i;
            let b = self.title_bar.buttons()[i].bounds();
            let inside = self.title_bar.buttons()[i].is_visible()
                && tb_local.x >= b.x
                && tb_local.x <= b.x + b.width
                && tb_local.y >= b.y
                && tb_local.y <= b.y + b.height;
            if inside && self.in_title_bar(pos) {
                hovered = Some(i);
            }
            dispatch_event_dyn(&mut self.title_bar, &[idx], &event, tb_local);
        }
        self.title_button_hover = hovered;
    }

    /// Remember the title-bar child path that consumed a press.
    pub(super) fn capture_title_bar_press(&mut self, path: Vec<usize>) {
        self.title_bar_capture = Some(path);
    }

    /// Deliver `event` (a release or a lost capture) to the title-bar child
    /// that took the press, if any. Returns `true` when one did.
    pub(super) fn release_title_bar_capture(&mut self, event: &Event, pos: Option<Point>) -> bool {
        let Some(path) = self.title_bar_capture.take() else {
            return false;
        };
        let tb_local = pos
            .map(|p| self.title_bar_local(p))
            .unwrap_or(Point::new(-1.0, -1.0));
        let translated = match event {
            Event::MouseUp {
                button, modifiers, ..
            } => Event::MouseUp {
                pos: tb_local,
                button: *button,
                modifiers: *modifiers,
            },
            other => other.clone(),
        };
        dispatch_event_dyn(&mut self.title_bar, &path, &translated, tb_local);
        crate::animation::request_draw();
        true
    }

    /// The tooltip of the added title-bar button under the pointer, if any.
    pub(super) fn hovered_title_button_tip(&self) -> Option<&str> {
        let i = self.title_button_hover?;
        self.title_bar.buttons().get(i)?.tooltip_text()
    }
}
