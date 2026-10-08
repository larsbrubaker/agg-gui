//! The runner's `ScrollIntoView` (agg-sharp `GuiAutomation/AutomationRunner.cs`):
//! find the named widget — the shallowest by count of on-screen ancestors
//! when several share the name, visible or not — and ask its nearest
//! scrolling ancestor to bring it into view through
//! [`Widget::scroll_rect_into_view`](agg_gui::Widget::scroll_rect_into_view).
//!
//! C#'s `ScrollAmount` parameter has only its default (`Minimum`) here; no
//! port needs `Center` yet. C# then nudges the scroller's width to force a
//! relayout; agg-gui requests one and the runner pumps a frame.

use agg_gui::{Rect, Size};

use super::named::WaitOpts;
use super::AutomationRunner;
use crate::driver::{FrameKind, UiDriver};
use crate::tree_query::{self, WidgetHandle};

impl AutomationRunner {
    /// C# `ScrollIntoView(widgetName)`. Nothing happens when the name is not
    /// found or no ancestor scrolls.
    pub fn scroll_into_view(&mut self, widget_name: &str) -> &mut Self {
        let opts = WaitOpts {
            only_visible: false,
            ..WaitOpts::default()
        };
        let hits = self.get_widgets_by_name(widget_name, &opts);
        let viewport = self.driver.logical_size();
        let root = self.driver.root();
        let depth = |handle: &WidgetHandle| {
            tree_query::parents(root, handle)
                .iter()
                .filter(|p| tree_query::actually_visible_on_screen(root, p, viewport))
                .count()
        };
        // C#'s OrderBy is stable: the first of equally shallow widgets wins.
        let Some(target) = hits
            .iter()
            .map(|hit| (depth(&hit.handle), &hit.handle))
            .min_by_key(|(d, _)| *d)
            .map(|(_, h)| h.clone())
        else {
            return self;
        };
        if self.scroll_nearest_scroller(&target) {
            agg_gui::animation::request_layout();
            self.pump_frame(FrameKind::Reactive);
        }
        self
    }

    /// Offer `target`'s rectangle to each ancestor, nearest first, until one
    /// scrolls. The rectangle is in the ancestor's local frame: both screen
    /// rectangles include every scroll offset above them.
    fn scroll_nearest_scroller(&mut self, target: &WidgetHandle) -> bool {
        let unbounded = Size::new(f64::INFINITY, f64::INFINITY);
        let root = self.driver.root();
        let Some(rect) = tree_query::placement(root, target, unbounded).map(|p| p.screen_rect)
        else {
            return false;
        };
        for ancestor in tree_query::parents(root, target) {
            let root = self.driver.root();
            let Some(origin) = tree_query::placement(root, &ancestor, unbounded) else {
                continue;
            };
            let local = Rect::new(
                rect.x - origin.screen_rect.x,
                rect.y - origin.screen_rect.y,
                rect.width,
                rect.height,
            );
            let Some(widget) = ancestor.widget_mut(self.driver.root_mut()) else {
                continue;
            };
            if widget.scroll_rect_into_view(local) {
                return true;
            }
        }
        false
    }
}
