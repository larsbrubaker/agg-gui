//! Drops outside the browser: the [`OutsideDrops`] hook through which an app takes a drag of the
//! browser's items onto its own views (a 3D view, a node editor). A config without one
//! (`BrowserConfig::drops` is `None`) keeps drags inside the browser: a release anywhere else
//! drops nothing.
//!
//! The browser asks the hook while its drag is outside the browser (over neither its list nor its
//! folder tree), with the items carried and the pointer in window coordinates; the hook says
//! whether a view of its takes the drop, and, whatever the answer, whether the browser hides the
//! picture its drag carries there (a view that shows the items itself, or an app whose drags show
//! their picture only over the browser). Without a hook, or where the hook leaves it shown, the
//! picture follows the pointer everywhere.
//!
//! When a drag comes back into the browser from the app's view, the hook is told to `cancel` (its
//! view puts itself back), unless it keeps its view while the browser is over it
//! ([`OutsideDrops::cancel_on_return`]: a browser panel lying over the app's view, whose preview
//! should stay put while the pointer crosses the panel). A drag that then ends inside the browser
//! drops on the folder there and tells the hook to `cancel`, or, over no folder, is the hook's
//! `release`.

use std::rc::Rc;

use agg_gui::Point;
use agg_gui_files::Item;

/// What is under a drag outside the browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutsideOver {
    /// Nothing of the app's that takes the items: a release here drops nothing.
    /// `hides_picture`: the browser hides the picture its drag carries here.
    NoTarget { hides_picture: bool },
    /// A view of the app's takes the items. `hides_picture`: the browser hides the picture its
    /// drag carries (the view shows the items itself).
    Target { hides_picture: bool },
}

impl OutsideOver {
    /// Whether the browser hides the drag's picture here.
    pub fn hides_picture(self) -> bool {
        match self {
            Self::NoTarget { hides_picture } | Self::Target { hides_picture } => hides_picture,
        }
    }
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

    /// Whether a drag coming back into the browser from the app's view cancels it there (the
    /// default). `false` keeps the app's view as the drag left it while the pointer is over the
    /// browser (see the module header for how the drag then ends).
    fn cancel_on_return(&self) -> bool {
        true
    }
}
