//! Port of agg-sharp `Tests/Agg.Tests/Agg.UI/PlatformSurfaceBoundsTests.cs`
//! over `App::layout` (`widget/app.rs`).
//!
//! Drag the window from a Retina display to a standard one and its surface
//! loses half its pixels in one step. If the widget tree laid out any taller
//! than the surface it just got, agg is y-up, so the overflow falls off the
//! top and the toolbars disappear under the title bar. C#'s `SystemWindow`
//! needs `SetBoundsFromPlatform` to beat its `MinimumSize` clamp; agg-gui's
//! `App::layout` sets the root to the surface the shell hands it, and the
//! root's `min_size` is only a layout hint, so the surface always wins. These
//! pin that. C#'s `AnOrdinaryBoundsAssignmentStillClamps` and
//! `AnOrdinaryWidgetStillGrowsToANewMinimum` pin the clamp-on-assignment seam
//! of `GuiWidget.LocalBounds`, which agg-gui does not have (`set_bounds`
//! never clamps; containers apply `min_size` when they lay their children
//! out), so they are not ported.

use super::*;
use crate::{DrawCtx, Event, EventResult, Rect};
use std::cell::Cell;
use std::rc::Rc;

/// The surface the shell reports, at a 1x display so device pixels and
/// logical units agree as they do in the C# test.
fn report_surface(app: &mut App, width: f64, height: f64) {
    crate::device_scale::set_device_scale(1.0);
    app.layout(Size::new(width, height));
}

/// A root whose minimum the test can raise after a surface report, as an
/// application does from its idle queue once it learns of the scale change.
struct ChangingMinimum {
    bounds: Rect,
    min: Rc<Cell<Size>>,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for ChangingMinimum {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn min_size(&self) -> Size {
        self.min.get()
    }
    fn layout(&mut self, available: Size) -> Size {
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

#[test]
fn a_platform_surface_smaller_than_the_minimum_is_honored() {
    // The Retina -> standard case exactly: the application sized its minimum
    // in device pixels for a 2x display, and the window is now on a 1x one
    // whose whole surface is smaller than that minimum.
    let root = FlexColumn::new().with_min_size(Size::new(900.0, 900.0));
    let mut app = App::new(Box::new(root));
    report_surface(&mut app, 1024.0, 1530.0);

    report_surface(&mut app, 1024.0, 765.0);

    assert_eq!(app.root().bounds().height, 765.0);
    assert_eq!(app.root().bounds().width, 1024.0);
}

#[test]
fn a_later_larger_surface_grows_the_window_back() {
    // Bypassing the clamp must not pin the window small - the next report is
    // just as authoritative as the one that shrank it (drag back to the
    // Retina display, or maximize).
    let root = FlexColumn::new().with_min_size(Size::new(900.0, 900.0));
    let mut app = App::new(Box::new(root));
    report_surface(&mut app, 1024.0, 1530.0);

    report_surface(&mut app, 1024.0, 765.0);
    report_surface(&mut app, 2048.0, 1530.0);

    assert_eq!(app.root().bounds().height, 1530.0);
    assert_eq!(app.root().bounds().width, 2048.0);
}

#[test]
fn raising_the_minimum_after_a_surface_report_does_not_inflate_the_window() {
    // The application learns about the scale change from the idle queue,
    // after the surface has already shrunk, and recomputes its minimum - if
    // that could inflate the bounds again, the toolbars go back under the
    // title bar. Once a host has stated the surface size, the surface size is
    // what the window lays out to.
    let min = Rc::new(Cell::new(Size::ZERO));
    let root = ChangingMinimum {
        bounds: Rect::default(),
        min: Rc::clone(&min),
        children: Vec::new(),
    };
    let mut app = App::new(Box::new(root));
    report_surface(&mut app, 1024.0, 1530.0);

    report_surface(&mut app, 1024.0, 765.0);
    min.set(Size::new(900.0, 900.0));
    // The next frame lays out at the same surface.
    report_surface(&mut app, 1024.0, 765.0);

    assert_eq!(app.root().bounds().height, 765.0);
    assert_eq!(app.root().min_size().height, 900.0);
}
