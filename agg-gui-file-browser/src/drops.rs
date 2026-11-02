//! Drops outside the browser: the [`OutsideDrops`] hook through which an app takes a drag of the
//! browser's items onto its own views (a 3D view, a node editor). A config without one
//! (`BrowserConfig::drops` is `None`) keeps drags inside the browser: a release anywhere else
//! drops nothing.
//!
//! The browser asks the hook while its drag is outside its own folders, with the items carried
//! and the pointer in window coordinates; the hook says whether a view of its takes the drop and
//! whether that view shows the items itself (so the browser hides the picture its drag carries).

use std::rc::Rc;

use agg_gui::Point;
use agg_gui_files::Item;

/// What is under a drag outside the browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutsideOver {
    /// Nothing of the app's that takes the items: a release here drops nothing.
    NoTarget,
    /// A view of the app's takes the items. `hides_picture`: the view shows them itself, so the
    /// browser hides the picture its drag carries.
    Target { hides_picture: bool },
}

/// An app's views that take a drag of the browser's items.
pub trait OutsideDrops {
    /// The drag carrying `items` is at `window_at`, outside the browser's folders.
    fn over(&self, items: &[Rc<dyn Item>], window_at: Point) -> OutsideOver;

    /// The drag carrying `items` was released at `window_at`; answers whether a view of the
    /// app's took them.
    fn release(&self, items: &[Rc<dyn Item>], window_at: Point) -> bool;

    /// The drag ended without a release the app hears (cancelled, or back inside the browser):
    /// a view showing the items while the drag was over it puts itself back.
    fn cancel(&self);
}
