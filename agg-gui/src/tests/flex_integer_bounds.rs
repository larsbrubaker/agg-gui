//! Flex layouts fit the child sizes they actually place. A child whose
//! natural size is fractional (a text field at one em plus insets, 22.67 px)
//! is placed at its rounded size, and the `FlexRow` / `FlexColumn` around it
//! reports that rounded size plus margins, as agg-sharp's
//! `GuiWidget.EnforceIntegerBounds` rounds a child's bounds before its
//! `FlowLayoutWidget` fits them. A child that opts out of integer bounds is
//! placed, and fitted, at its exact size.

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor};
use crate::widget::Widget;
use crate::widgets::{FlexColumn, FlexRow};

/// Reports a fixed, fractional natural size, with a margin and an
/// integer-bounds flag.
struct Frac {
    bounds: Rect,
    natural: Size,
    margin: Insets,
    enforce: bool,
}

impl Frac {
    fn boxed(w: f64, h: f64, margin: Insets, enforce: bool) -> Box<dyn Widget> {
        Box::new(Self {
            bounds: Rect::default(),
            natural: Size::new(w, h),
            margin,
            enforce,
        })
    }
}

impl Widget for Frac {
    fn type_name(&self) -> &'static str {
        "Frac"
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
        unreachable!("Frac has no children")
    }
    fn margin(&self) -> Insets {
        self.margin
    }
    fn h_anchor(&self) -> HAnchor {
        HAnchor::FIT
    }
    fn v_anchor(&self) -> VAnchor {
        VAnchor::FIT
    }
    fn enforce_integer_bounds(&self) -> bool {
        self.enforce
    }
    fn measure_min_height(&self, _available_w: f64) -> f64 {
        self.natural.height
    }
    fn layout(&mut self, _available: Size) -> Size {
        self.natural
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Top 9, bottom 9, left 4, right 4.
fn margins() -> Insets {
    Insets {
        left: 4.0,
        right: 4.0,
        top: 9.0,
        bottom: 9.0,
    }
}

#[test]
fn rust_only_flex_row_fits_its_child_rounded_height() {
    let mut row = FlexRow::new().add(Frac::boxed(50.0, 22.67, margins(), true));
    let size = row.layout(Size::new(400.0, 1000.0));
    let child = row.children()[0].bounds();
    assert_eq!(child.height, 23.0);
    // The row is the rounded child plus its margins, not 40.67.
    assert_eq!(size.height, 41.0);
    // Laid out at that height, the child keeps its full box: nothing above
    // its top margin is cut off.
    row.layout(Size::new(400.0, size.height));
    let child = row.children()[0].bounds();
    assert_eq!(child.y, 9.0);
    assert_eq!(child.y + child.height + 9.0, size.height);
}

#[test]
fn rust_only_flex_row_fit_width_sums_rounded_child_widths() {
    let mut row = FlexRow::new()
        .with_gap(0.0)
        .with_fit_width(true)
        .add(Frac::boxed(10.4, 10.0, margins(), true))
        .add(Frac::boxed(10.4, 10.0, margins(), true));
    let size = row.layout(Size::new(400.0, 1000.0));
    let (a, b) = (row.children()[0].bounds(), row.children()[1].bounds());
    assert_eq!((a.x, a.width), (4.0, 10.0));
    // The second child starts after the first's rounded width.
    assert_eq!((b.x, b.width), (22.0, 10.0));
    assert_eq!(size.width, 36.0);
}

#[test]
fn rust_only_flex_row_keeps_exact_height_for_a_child_without_integer_bounds() {
    let mut row = FlexRow::new().add(Frac::boxed(50.0, 22.67, margins(), false));
    let size = row.layout(Size::new(400.0, 1000.0));
    assert_eq!(row.children()[0].bounds().height, 22.67);
    assert_eq!(size.height, 22.67 + 18.0);
}

#[test]
fn rust_only_flex_column_fits_its_children_rounded_heights() {
    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .add(Frac::boxed(50.0, 22.67, margins(), true))
        .add(Frac::boxed(50.0, 22.67, margins(), true));
    let size = col.layout(Size::new(400.0, 1000.0));
    // Two rounded 23 px children with 18 px of margin each.
    assert_eq!(size.height, 82.0);
    // The content-fit measure (a tight-fit `Window` sizes from it) agrees,
    // so the window holds the column without scrolling.
    assert_eq!(col.measure_min_height(400.0), 82.0);
    col.layout(Size::new(400.0, size.height));
    let (top, bottom) = (col.children()[0].bounds(), col.children()[1].bounds());
    assert_eq!((top.y, top.height), (50.0, 23.0));
    assert_eq!((bottom.y, bottom.height), (9.0, 23.0));
}

#[test]
fn rust_only_flex_column_fit_width_uses_the_rounded_child_width() {
    let mut col =
        FlexColumn::new()
            .with_fit_width(true)
            .add(Frac::boxed(22.67, 10.0, margins(), true));
    let size = col.layout(Size::new(400.0, 1000.0));
    assert_eq!(col.children()[0].bounds().width, 23.0);
    assert_eq!(size.width, 31.0);
}

#[test]
fn rust_only_flex_column_keeps_exact_sizes_for_a_child_without_integer_bounds() {
    let mut col =
        FlexColumn::new()
            .with_fit_width(true)
            .add(Frac::boxed(22.67, 22.67, margins(), false));
    let size = col.layout(Size::new(400.0, 1000.0));
    let child = col.children()[0].bounds();
    assert_eq!((child.width, child.height), (22.67, 22.67));
    assert_eq!(size, Size::new(22.67 + 8.0, 22.67 + 18.0));
}
