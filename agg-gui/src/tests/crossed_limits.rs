//! Crossed size limits never panic a layout. A widget may report a
//! `min_size` above its `max_size` (agg-sharp's `GuiWidget` setters keep the
//! two ordered, but a Rust `Widget` reports whatever it likes), and a slot can
//! be smaller than a child's floor. Every layout that holds a size to limits
//! goes through `layout_props::clamp_to_limits`, where the floor wins, instead
//! of `f64::clamp`, which panics when min > max. One test per layout site.

use super::*;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{clamp_to_limits, HAnchor, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::{
    AbsoluteLayout, FlexColumn, FlexRow, Resize, SizedBox, Stack, TabView, Window,
};

/// Reports `natural` from layout and carries crossed limits and anchors.
struct Crossed {
    bounds: Rect,
    natural: Size,
    min: Size,
    max: Size,
    h: HAnchor,
    v: VAnchor,
}

impl Crossed {
    /// min 120 x 30 above max 100 x 20 on both axes.
    fn new() -> Self {
        Self {
            bounds: Rect::default(),
            natural: Size::new(50.0, 10.0),
            min: Size::new(120.0, 30.0),
            max: Size::new(100.0, 20.0),
            h: HAnchor::FIT,
            v: VAnchor::FIT,
        }
    }
    fn anchored(mut self, h: HAnchor, v: VAnchor) -> Self {
        self.h = h;
        self.v = v;
        self
    }
}

impl Widget for Crossed {
    fn type_name(&self) -> &'static str {
        "Crossed"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &[]
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        unreachable!("Crossed has no children")
    }
    fn min_size(&self) -> Size {
        self.min
    }
    fn max_size(&self) -> Size {
        self.max
    }
    fn h_anchor(&self) -> HAnchor {
        self.h
    }
    fn v_anchor(&self) -> VAnchor {
        self.v
    }
    fn layout(&mut self, _available: Size) -> Size {
        self.natural
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

fn crossed() -> Box<dyn Widget> {
    Box::new(Crossed::new())
}

fn stretched() -> Box<dyn Widget> {
    Box::new(Crossed::new().anchored(HAnchor::STRETCH, VAnchor::STRETCH))
}

#[test]
fn rust_only_clamp_to_limits_lets_the_floor_win() {
    assert_eq!(clamp_to_limits(50.0, 10.0, 100.0), 50.0);
    assert_eq!(clamp_to_limits(5.0, 10.0, 100.0), 10.0);
    assert_eq!(clamp_to_limits(500.0, 10.0, 100.0), 100.0);
    assert_eq!(clamp_to_limits(50.0, 120.0, 100.0), 120.0);
    assert_eq!(clamp_to_limits(500.0, 120.0, 100.0), 120.0);
}

#[test]
fn rust_only_widget_base_clamp_size_lets_the_floor_win() {
    let mut base = WidgetBase::new();
    base.min_size = Size::new(120.0, 30.0);
    base.max_size = Size::new(100.0, 20.0);
    assert_eq!(
        base.clamp_size(Size::new(400.0, 400.0)),
        Size::new(120.0, 30.0)
    );
}

#[test]
fn rust_only_flex_column_main_axis_lets_the_floor_win() {
    let mut col = FlexColumn::new().add(crossed());
    col.layout(Size::new(400.0, 400.0));
    assert_eq!(col.children()[0].bounds().height, 30.0);
}

#[test]
fn rust_only_flex_column_cross_axis_lets_the_floor_win() {
    let mut col = FlexColumn::new().add(crossed()).add(stretched());
    col.layout(Size::new(400.0, 400.0));
    assert_eq!(col.children()[0].bounds().width, 120.0);
    assert_eq!(col.children()[1].bounds().width, 120.0);
}

#[test]
fn rust_only_flex_row_cross_axis_lets_the_floor_win() {
    let mut row = FlexRow::new().add(crossed()).add(stretched());
    row.layout(Size::new(400.0, 400.0));
    assert_eq!(row.children()[0].bounds().height, 30.0);
    assert_eq!(row.children()[1].bounds().height, 30.0);
}

#[test]
fn rust_only_stack_aligned_child_lets_the_floor_win() {
    let mut stack = Stack::new().add_aligned(crossed());
    stack.layout(Size::new(400.0, 400.0));
    let b = stack.children()[0].bounds();
    assert_eq!((b.width, b.height), (120.0, 30.0));
}

#[test]
fn rust_only_sized_box_lets_the_floor_win() {
    let mut fit = SizedBox::new().with_child(crossed());
    fit.layout(Size::new(400.0, 400.0));
    let b = fit.children()[0].bounds();
    assert_eq!((b.width, b.height), (120.0, 30.0));

    let mut stretch = SizedBox::new().with_child(stretched());
    stretch.layout(Size::new(400.0, 400.0));
    assert_eq!(stretch.children()[0].bounds().width, 120.0);

    // The box's own crossed limits, in layout and in measure_min_height.
    let mut own = SizedBox::new()
        .with_child(Box::new(SizedBox::fixed(10.0, 10.0)))
        .with_min_size(Size::new(0.0, 60.0))
        .with_max_size(Size::new(f64::MAX, 40.0));
    assert_eq!(own.measure_min_height(400.0), 60.0);
    assert_eq!(own.layout(Size::new(400.0, 400.0)).height, 60.0);
}

#[test]
fn rust_only_resize_lets_the_floor_win() {
    let mut resize = Resize::new(Box::new(SizedBox::fixed(10.0, 10.0)))
        .with_default_size(Size::new(400.0, 400.0))
        .with_min_size_hint(Size::new(120.0, 30.0))
        .with_max_size_hint(Size::new(100.0, 20.0));
    let size = resize.layout(Size::new(500.0, 500.0));
    assert_eq!(size, Size::new(120.0, 30.0));
}

#[test]
fn rust_only_absolute_layout_child_lets_the_floor_win() {
    let mut abs = AbsoluteLayout::new().add(crossed());
    abs.layout(Size::new(400.0, 400.0));
    let b = abs.children()[0].bounds();
    assert_eq!((b.width, b.height), (120.0, 30.0));
}

#[test]
fn rust_only_tab_view_sidebar_survives_a_narrow_view() {
    // 0.8 of 100 px is below the sidebar's 160 px floor; the floor wins.
    let show = std::rc::Rc::new(std::cell::Cell::new(true));
    let mut tv = TabView::new(font()).with_sidebar(crossed(), show);
    tv.layout(Size::new(100.0, 300.0));
}

#[test]
fn rust_only_window_content_floor_beats_a_short_window() {
    // The content's 30 px floor is taller than the window's content area.
    let mut win = Window::new("W", font(), crossed());
    win.set_bounds(Rect::new(0.0, 0.0, 200.0, 40.0));
    win.layout(Size::new(800.0, 600.0));
    assert_eq!(win.children()[0].bounds().height, 30.0);
}
