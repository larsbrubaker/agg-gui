//! Room a `Solid` [`ScrollView`] bar takes beside the content: only a bar
//! that shows takes any (agg-sharp's `ScrollBar.ReservesSpace`, `Visible &&
//! !floating`), the other bar's room counts toward an axis's overflow only
//! when that bar shows (`HasVerticalOverflow` / `MaxScrollFromLeft` against
//! `ScrollArea.DeviceMargin`), and a single layout pass already lays the
//! content out at the width the reserve leaves.
//!
//! The style is agg-sharp's classic bar as MatterCAD uses it: 15 wide,
//! `Solid`, no margins and no resize-grip guard.

use std::cell::Cell;
use std::rc::Rc;

use crate::{
    DrawCtx, Event, EventResult, Rect, ScrollBarKind, ScrollBarStyle, ScrollBarVisibility,
    ScrollView, Size, Widget,
};

const BAR: f64 = 15.0;

fn classic() -> ScrollBarStyle {
    ScrollBarStyle {
        bar_width: BAR,
        floating_width: BAR,
        outer_margin: 0.0,
        inner_margin: 0.0,
        kind: ScrollBarKind::Solid,
        edge_guard: 0.0,
        ..ScrollBarStyle::default()
    }
}

/// Content of a given width (or the offered width when `None`) and height
/// that records the width each layout offered it.
struct Probe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    width: Option<f64>,
    height: Rc<Cell<f64>>,
    offered: Rc<Cell<f64>>,
}

impl Widget for Probe {
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
    fn layout(&mut self, available: Size) -> Size {
        self.offered.set(available.width);
        Size::new(self.width.unwrap_or(available.width), self.height.get())
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}

struct Setup {
    view: ScrollView,
    height: Rc<Cell<f64>>,
    offered: Rc<Cell<f64>>,
}

fn setup(width: Option<f64>, height: f64, horizontal: bool) -> Setup {
    let height = Rc::new(Cell::new(height));
    let offered = Rc::new(Cell::new(f64::NAN));
    let probe = Probe {
        bounds: Rect::default(),
        children: Vec::new(),
        width,
        height: Rc::clone(&height),
        offered: Rc::clone(&offered),
    };
    let view = ScrollView::new(Box::new(probe))
        .horizontal(horizontal)
        .with_style(classic())
        .with_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    Setup {
        view,
        height,
        offered,
    }
}

/// `(vertical showing, horizontal showing, viewport size)` after one layout
/// at `w` x `h`.
fn lay_out(view: &mut ScrollView, w: f64, h: f64) -> (bool, bool, Size) {
    view.layout(Size::new(w, h));
    let p = view.scroll_bar_properties();
    (
        p.vertical.showing,
        p.horizontal.showing,
        Size::new(p.viewport.width, p.viewport.height),
    )
}

#[test]
fn content_within_one_bar_of_the_view_keeps_the_full_width_when_the_other_axis_is_off() {
    // MatterCAD's design tree: 96 of content in a 101.5 tall view. Nothing
    // scrolls, and there is no horizontal bar whose room could make it.
    let mut s = setup(None, 96.0, false);
    let (v, h, viewport) = lay_out(&mut s.view, 200.0, 101.5);
    assert!(!v && !h);
    assert_eq!(viewport, Size::new(200.0, 101.5));
    assert_eq!(s.offered.get(), 200.0);
    assert_eq!(s.view.children()[0].bounds().width, 200.0);
    assert_eq!(s.view.max_scroll_value(), 0.0);
}

#[test]
fn the_first_layout_pass_lays_overflowing_content_out_beside_the_bar() {
    let mut s = setup(None, 500.0, false);
    let (v, _, viewport) = lay_out(&mut s.view, 200.0, 100.0);
    assert!(v);
    assert_eq!(viewport.width, 200.0 - BAR);
    // The content was measured at the width it is shown at, not the full
    // width the previous (no) content left.
    assert_eq!(s.offered.get(), 200.0 - BAR);
    assert_eq!(s.view.children()[0].bounds().width, 200.0 - BAR);
}

#[test]
fn content_shrinking_to_fit_gets_the_bar_room_back_in_the_same_pass() {
    let mut s = setup(None, 500.0, false);
    lay_out(&mut s.view, 200.0, 100.0);
    s.height.set(80.0);
    let (v, _, viewport) = lay_out(&mut s.view, 200.0, 100.0);
    assert!(!v);
    assert_eq!(viewport.width, 200.0);
    assert_eq!(s.offered.get(), 200.0);
}

#[test]
fn content_within_one_bar_on_both_axes_shows_neither_bar() {
    // Each bar would bring up the other, but neither axis overflows on its
    // own; agg-sharp starts with no bars and so never shows one.
    let mut s = setup(Some(195.0), 95.0, true);
    let (v, h, viewport) = lay_out(&mut s.view, 200.0, 100.0);
    assert!(!v && !h);
    assert_eq!(viewport, Size::new(200.0, 100.0));
}

#[test]
fn one_overflowing_axis_brings_up_the_other_bar_when_its_room_makes_that_overflow() {
    // Too wide: the horizontal bar shows, and its room makes the 95 tall
    // content overflow the 85 left.
    let mut s = setup(Some(210.0), 95.0, true);
    let (v, h, viewport) = lay_out(&mut s.view, 200.0, 100.0);
    assert!(v && h);
    assert_eq!(viewport, Size::new(200.0 - BAR, 100.0 - BAR));

    // Too tall: the vertical bar shows and its room makes 195 overflow 185.
    let mut s = setup(Some(195.0), 110.0, true);
    let (v, h, viewport) = lay_out(&mut s.view, 200.0, 100.0);
    assert!(v && h);
    assert_eq!(viewport, Size::new(200.0 - BAR, 100.0 - BAR));

    // Too tall, but narrow enough to fit beside the vertical bar.
    let mut s = setup(Some(180.0), 110.0, true);
    let (v, h, viewport) = lay_out(&mut s.view, 200.0, 100.0);
    assert!(v && !h);
    assert_eq!(viewport, Size::new(200.0 - BAR, 100.0));
}

#[test]
fn content_overflowing_both_axes_reserves_both_bars() {
    let mut s = setup(Some(600.0), 500.0, true);
    let (v, h, viewport) = lay_out(&mut s.view, 200.0, 100.0);
    assert!(v && h);
    assert_eq!(viewport, Size::new(200.0 - BAR, 100.0 - BAR));
}
