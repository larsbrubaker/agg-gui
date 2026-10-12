//! agg-sharp's `Tests/Agg.Tests/Agg.UI/ScrollingAreaDeviceScaleTests.cs`, on
//! [`ScrollView`]: the content's margin, the thumb ratio and the bar's
//! visibility come out the same on a 1x and a 2x display.
//!
//! agg-sharp lays out in device pixels, so its fixtures size the view and the
//! content `x scale` and the margin (design units) had to be scaled to match.
//! agg-gui lays out in logical units at every scale (the renderer applies
//! [`device_scale`](crate::device_scale::device_scale)), so the C#
//! `200 * scale` view is a 200-unit view here, the `5`-unit margin is 5
//! units, and "twice as far in device pixels at 2x" is "the same distance in
//! logical units". Each test runs with the device scale set, as C#'s does,
//! and checks the logical result is the device result divided by the scale.
//!
//! agg-gui's `ScrollView` has no scroll-area margin of its own: the margin is
//! carried by the content (a block inset by the margin on every side,
//! [`Inset`]), which is what the scroll clamp then measures. The C# view
//! clamps sideways scrolling without a horizontal bar; agg-gui lets content
//! move sideways only on a horizontal view, so the fixture turns that on.
//! The C# `[Arguments(1.0)] [Arguments(2.0)]` cases run in one test, each
//! scale in turn.

use std::cell::Cell;
use std::rc::Rc;

use super::scroll_fixtures::Fixed;
use crate::device_scale::{device_scale, set_device_scale};
use crate::{
    DrawCtx, Event, EventResult, Rect, ScrollBarStyle, ScrollBarVisibility, ScrollView, Size,
    Widget,
};

const MARGIN_DESIGN: f64 = 5.0;

/// Content of a `width` x `height` block inset by `margin` on every side.
struct Inset {
    bounds: Rect,
    margin: f64,
    children: Vec<Box<dyn Widget>>,
}

impl Inset {
    fn new(width: f64, height: f64, margin: f64) -> Self {
        Self {
            bounds: Rect::default(),
            margin,
            children: vec![Box::new(Fixed::new(width, height, None))],
        }
    }
}

impl Widget for Inset {
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
    fn layout(&mut self, _available: Size) -> Size {
        let m = self.margin;
        let block = self.children[0].layout(Size::new(0.0, 0.0));
        self.children[0].set_bounds(Rect::new(m, m, block.width, block.height));
        Size::new(block.width + 2.0 * m, block.height + 2.0 * m)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Restores the device scale when a test ends, passing or not.
struct ScaleGuard(f64);

impl Drop for ScaleGuard {
    fn drop(&mut self) {
        set_device_scale(self.0);
    }
}

/// A 200 x 200 view holding one content block with the design margin on
/// every side, built after the scale is set, and its horizontal offset.
fn make_scrollable(
    content_width: f64,
    content_height: f64,
    show: ScrollBarVisibility,
) -> (ScrollView, Rc<Cell<f64>>) {
    let h_offset = Rc::new(Cell::new(0.0));
    let mut scroll = ScrollView::new(Box::new(Inset::new(
        content_width,
        content_height,
        MARGIN_DESIGN,
    )))
    .horizontal(true)
    .with_style(ScrollBarStyle::solid())
    .with_bar_visibility(show)
    .with_h_offset_cell(Rc::clone(&h_offset));
    lay_out(&mut scroll);
    (scroll, h_offset)
}

fn lay_out(scroll: &mut ScrollView) {
    scroll.set_bounds(Rect::new(0.0, 0.0, 200.0, 200.0));
    let _ = scroll.layout(Size::new(200.0, 200.0));
}

/// A logical rect in device pixels at `scale`.
fn device(r: Rect, scale: f64) -> Rect {
    Rect::new(r.x * scale, r.y * scale, r.width * scale, r.height * scale)
}

/// The content block's rect in the view's space.
fn block(scroll: &ScrollView) -> Rect {
    let content = scroll.children()[0].bounds();
    let b = scroll.children()[0].children()[0].bounds();
    Rect::new(content.x + b.x, content.y + b.y, b.width, b.height)
}

#[test]
fn scroll_ends_leave_exactly_the_device_margin() {
    let _guard = ScaleGuard(device_scale());
    for scale in [1.0, 2.0] {
        set_device_scale(scale);
        let (mut scroll, h_offset) =
            make_scrollable(300.0, 400.0, ScrollBarVisibility::AlwaysHidden);
        // C#'s device-pixel expectations; agg-gui's logical results are
        // compared after converting them to device pixels (x scale)
        let margin = MARGIN_DESIGN * scale;
        let view = device(scroll.bounds(), scale);

        // the start of the content: scrolled to the top and the left
        scroll.set_scroll_offset(-100_000.0);
        h_offset.set(-100_000.0);
        lay_out(&mut scroll);
        let content = device(block(&scroll), scale);
        assert!(
            (content.y + content.height - (view.height - margin)).abs() < 0.001,
            "at the top the content sits the device margin below the view's top \
             (scale {scale}, content {content:?})"
        );
        assert!(
            (content.x - margin).abs() < 0.001,
            "at the left the content sits the device margin inside the view's left \
             (scale {scale}, content {content:?})"
        );

        // and the other way: the end of the content
        scroll.set_scroll_offset(100_000.0);
        h_offset.set(100_000.0);
        lay_out(&mut scroll);
        let content = device(block(&scroll), scale);
        assert!(
            (content.y - margin).abs() < 0.001,
            "at the bottom the content sits the device margin above the view's bottom \
             (scale {scale}, content {content:?})"
        );
        assert!(
            (content.x + content.width - (view.width - margin)).abs() < 0.001,
            "at the right the content sits the device margin inside the view's right \
             (scale {scale}, content {content:?})"
        );
    }
}

/// `(view ratio (x, y), half way top from the view's top in device pixels,
/// ratio read back)`.
fn measure_ratios(scale: f64) -> ((f64, f64), f64, f64) {
    set_device_scale(scale);
    let (mut scroll, _) = make_scrollable(300.0, 400.0, ScrollBarVisibility::AlwaysHidden);

    let v = scroll.vertical_bar_report();
    let h = scroll.horizontal_bar_report();
    let view_ratio = (h.viewport / h.content, v.viewport / v.content);

    // half way down
    let max = scroll.max_scroll_value();
    scroll.set_scroll_offset(max * 0.5);
    lay_out(&mut scroll);
    let content = scroll.children()[0].bounds();
    let half_way_top = (scroll.bounds().height - (content.y + content.height)) * scale;

    // a quarter of the way through the travel, measured from the two ends the
    // clamp allows
    scroll.set_scroll_offset(-100_000.0);
    lay_out(&mut scroll);
    let top_end = scroll.scroll_offset();
    scroll.set_scroll_offset(100_000.0);
    lay_out(&mut scroll);
    let bottom_end = scroll.scroll_offset();
    scroll.set_scroll_offset(top_end + (bottom_end - top_end) * 0.25);
    lay_out(&mut scroll);
    let read_back = scroll.scroll_offset() / scroll.max_scroll_value();

    (view_ratio, half_way_top, read_back)
}

#[test]
fn ratios_at_two_x_match_one_x() {
    let _guard = ScaleGuard(device_scale());
    let (one_view, one_half, one_back) = measure_ratios(1.0);
    let (two_view, two_half, two_back) = measure_ratios(2.0);

    assert!(
        (two_view.1 - one_view.1).abs() < 0.0001,
        "the thumb is the same share of the track at any scale"
    );
    assert!((two_view.0 - one_view.0).abs() < 0.0001);
    assert!(
        (two_half - one_half * 2.0).abs() < 0.001,
        "half way down lands twice as far from the view's top at 2x ({two_half} vs {one_half})"
    );
    assert!(
        (two_back - one_back).abs() < 0.0001,
        "the ratio read back from a scrolled position is the same at any scale"
    );
}

#[test]
fn content_taller_than_the_view_only_by_its_margins_shows_the_bar() {
    let _guard = ScaleGuard(device_scale());
    for scale in [1.0, 2.0] {
        set_device_scale(scale);
        // 6 units short of the view: fits on its own, but overflows with its
        // 2 x 5 of margin
        let (scroll, _) = make_scrollable(150.0, 194.0, ScrollBarVisibility::VisibleWhenNeeded);
        assert!(
            scroll.vertical_bar_report().shown,
            "the margins are part of how far the content moves, so there is something to \
             scroll to (scale {scale})"
        );
    }
}
