//! Size limits reach the layout that owns the slot: a `Named` wrapper reports
//! its child's `min_size` / `max_size` (it has no limits of its own, so a row
//! would otherwise see the defaults), and a `FlexRow` holds each non-flex
//! child to its `max_size` on the main axis, as agg-sharp's `FlowLayoutWidget`
//! respects `MaximumSize`.

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::widget::Widget;
use crate::widgets::{FlexRow, Named};

/// A widget that takes every pixel of width it is offered (as a text field
/// does) and leaves its limits for the parent to apply.
struct Greedy {
    bounds: Rect,
    min: Size,
    max: Size,
}

impl Greedy {
    fn new(min: Size, max: Size) -> Self {
        Self {
            bounds: Rect::default(),
            min,
            max,
        }
    }
}

impl Widget for Greedy {
    fn type_name(&self) -> &'static str {
        "Greedy"
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
        unreachable!("Greedy has no children")
    }
    fn min_size(&self) -> Size {
        self.min
    }
    fn max_size(&self) -> Size {
        self.max
    }
    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, 20.0)
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn capped(width: f64) -> Greedy {
    Greedy::new(Size::ZERO, Size::new(width, f64::MAX))
}

#[test]
fn rust_only_named_reports_its_childs_size_limits() {
    let named = Named::new(
        "field",
        Box::new(Greedy::new(Size::new(40.0, 10.0), Size::new(100.0, 30.0))),
    );
    assert_eq!(named.min_size(), Size::new(40.0, 10.0));
    assert_eq!(named.max_size(), Size::new(100.0, 30.0));

    // With its child taken away the wrapper has no limits.
    let mut empty = Named::new("field", Box::new(capped(100.0)));
    empty.children_mut().clear();
    assert_eq!(empty.min_size(), Size::ZERO);
    assert_eq!(empty.max_size(), Size::MAX);
}

#[test]
fn rust_only_a_named_capped_child_keeps_its_width_in_a_row() {
    let mut row = FlexRow::new().add(Box::new(Named::new("field", Box::new(capped(100.0)))));
    row.layout(Size::new(400.0, 50.0));
    let named = &row.children()[0];
    assert_eq!(named.bounds().width, 100.0);
    assert_eq!(named.children()[0].bounds().width, 100.0);
}

#[test]
fn rust_only_flex_row_clamps_a_fixed_child_to_its_max_width() {
    let mut row = FlexRow::new()
        .add(Box::new(capped(100.0)))
        .add(Box::new(capped(50.0)));
    row.layout(Size::new(400.0, 50.0));
    let (a, b) = (row.children()[0].bounds(), row.children()[1].bounds());
    assert_eq!(a.width, 100.0);
    assert_eq!(b.width, 50.0);
    assert!(b.x >= a.x + a.width, "the second child starts after the first");
}

#[test]
fn rust_only_flex_row_lets_the_floor_win_when_limits_cross() {
    // min above max: the floor wins, as `WidgetBase::clamp_size` decides, and
    // the row must not panic (`f64::clamp` does when min > max).
    let mut row = FlexRow::new().add(Box::new(Greedy::new(
        Size::new(120.0, 0.0),
        Size::new(100.0, f64::MAX),
    )));
    row.layout(Size::new(400.0, 50.0));
    assert_eq!(row.children()[0].bounds().width, 120.0);
}
