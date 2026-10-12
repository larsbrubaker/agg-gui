// The runtime options of a `Window` as agg-sharp's `WindowWidget` exposes them
// (`Collapsed`, the collapse button, `Title`'s label, whether the grab handles
// are live), read and set directly. The flags themselves are driven by the
// builder and the runtime cells in `builder.rs`, applied each `layout()` in
// `widget_impl.rs`; the collapse math is `Window::toggle_collapse` and the
// handle hit-test `Window::resize_dir` (both in `window.rs`).

use super::*;

impl Window {
    /// Whether the window is folded up to its title bar.
    pub fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    /// Fold the window up to its title bar (`true`) or back out to the height
    /// it had (`false`), keeping the top edge where it is either way (agg-sharp
    /// `WindowWidget.Collapsed`). Setting the state it already has does
    /// nothing.
    pub fn set_collapsed(&mut self, collapsed: bool) {
        if collapsed != self.collapsed {
            self.toggle_collapse();
            self.backbuffer.invalidate();
            crate::animation::request_draw();
        }
    }

    /// Whether the edge and corner grab handles resize the window now: the
    /// window is resizable and neither collapsed nor auto-sized (agg-sharp's
    /// `GrabControl` visibility).
    pub fn resize_handles_active(&self) -> bool {
        self.resizable && !self.collapsed && !self.auto_size
    }

    /// The text the title bar shows, which follows a wired title cell (see
    /// [`Window::with_title_cell`]) while [`Window::title`] keeps the
    /// window's identity.
    pub fn title_bar_text(&self) -> String {
        self.title_bar.title_text()
    }

    /// Whether the title bar offers the collapse chevron.
    pub fn collapse_button_shown(&self) -> bool {
        self.chrome && self.title_bar.chevron().is_enabled()
    }

    /// The collapse chevron's rectangle in window-local coordinates (Y-up),
    /// or `None` while it is not shown.
    pub fn collapse_button_rect(&self) -> Option<Rect> {
        if !self.collapse_button_shown() {
            return None;
        }
        let bar = self.title_bar.bounds();
        let c = self.title_bar.chevron().bounds();
        Some(Rect::new(bar.x + c.x, bar.y + c.y, c.width, c.height))
    }
}
