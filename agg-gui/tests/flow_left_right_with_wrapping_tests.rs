//! agg-sharp `Tests/Agg.Tests/Agg.UI/FlowLeftRightWithWrappingTests.cs`: where a
//! `FlowLeftRightWithWrapping` breaks its rows and where it draws their items,
//! at device scale 1 and 2 (each C# `[Arguments]` row is one iteration of the
//! test's loop).
//!
//! C# measures in device pixels, so its widths are `50 * scale` and its
//! expectations `* scale`; agg-gui lays out in logical units at every scale, so
//! the same layout reads the unscaled numbers here. The parent is a real
//! `FlexColumn` (C#'s plain `GuiWidget` parent), which gives a stretching
//! child its width less its margin.
//!
//! `AFlowNarrowedWhileItGrowsStillEnclosesEveryRow` narrows the flow from a
//! C# `BoundsChanged` handler while the flow re-wraps; agg-gui has no bounds
//! events (layout is a function of the width it is given), so the same
//! narrowing runs as successive layouts.

use agg_gui::{
    set_device_scale, FlexColumn, FlowLeftRightWithWrapping, HAnchor, HardBreak, Insets, Size,
    Spacer, Widget,
};

const SCALES: [f64; 2] = [1.0, 2.0];

fn item(width: f64, height: f64) -> Box<dyn Widget> {
    let size = Size::new(width, height);
    Box::new(Spacer::new().with_min_size(size).with_max_size(size))
}

fn new_flow(item_count: usize) -> FlowLeftRightWithWrapping {
    let mut flow = FlowLeftRightWithWrapping::new();
    for _ in 0..item_count {
        flow.add_child(item(50.0, 20.0));
    }
    flow
}

/// The flow in a parent `width` x `height` wide, laid out (C#
/// `parent.AddChild(flow)`).
fn in_parent(flow: FlowLeftRightWithWrapping, width: f64) -> FlexColumn {
    let mut parent = FlexColumn::new()
        .with_inner_padding(Insets::ZERO)
        .with_h_anchor(HAnchor::STRETCH);
    parent.push(Box::new(flow), 0.0);
    parent.layout(Size::new(width, 500.0));
    parent
}

fn flow_of(parent: &FlexColumn) -> &dyn Widget {
    parent.children()[0].as_ref()
}

fn filled_rows(flow: &dyn Widget) -> usize {
    flow.children()
        .iter()
        .filter(|row| !row.children().is_empty())
        .count()
}

/// Where `item` (child `index` of row `row`) is drawn in the flow: its row's
/// place plus its place in the row.
fn drawn_left(flow: &dyn Widget, row: usize, index: usize) -> f64 {
    let row = &flow.children()[row];
    row.bounds().x + row.children()[index].bounds().x
}

fn drawn_right(flow: &dyn Widget, row: usize, index: usize) -> f64 {
    let row_widget = &flow.children()[row];
    drawn_left(flow, row, index) + row_widget.children()[index].bounds().width
}

/// ContentWidth - every item's width and margin end to end - is known before
/// the flow has wrapped, so a host can size a window to fit it on one row.
#[test]
fn content_width_is_known_before_any_wrap() {
    for scale in SCALES {
        set_device_scale(scale);
        let mut flow = FlowLeftRightWithWrapping::new();
        let items: [(f64, Insets); 3] = [
            (50.0, Insets::symmetric(2.0, 0.0)),
            (30.0, Insets::symmetric(4.0, 0.0)),
            (70.0, Insets::ZERO),
        ];
        for (width, margin) in items {
            let size = Size::new(width, 20.0);
            flow.add_child(Box::new(
                Spacer::new()
                    .with_min_size(size)
                    .with_max_size(size)
                    .with_margin(margin),
            ));
        }

        let expected: f64 = items.iter().map(|(w, m)| w + m.left + m.right).sum();
        assert_eq!(expected, 50.0 + 4.0 + 30.0 + 8.0 + 70.0);
        // the flow has no parent and has never wrapped
        assert_eq!(flow.content_width(), expected, "at {scale}x");
    }
    set_device_scale(1.0);
}

/// Widening the parent re-wraps the flow at the new width, and narrowing it
/// wraps it again.
#[test]
fn resizing_the_parent_rewraps_at_the_new_width() {
    for scale in SCALES {
        set_device_scale(scale);
        let mut parent = in_parent(new_flow(4), 206.0);
        // 200 units of items plus 12 of row padding and margin do not fit in 206
        assert_eq!(filled_rows(flow_of(&parent)), 2, "at {scale}x");

        parent.layout(Size::new(212.0, 500.0));
        // they fit in 212
        assert_eq!(filled_rows(flow_of(&parent)), 1, "at {scale}x");

        parent.layout(Size::new(160.0, 500.0));
        // three items and the row's 12 units fit in 160 and the fourth wraps
        assert_eq!(filled_rows(flow_of(&parent)), 2, "at {scale}x");
    }
    set_device_scale(1.0);
}

/// A bordered row's border takes room from its items: rows after the first
/// carry RowBorder, so their items wrap before they would draw into it.
#[test]
fn a_rows_border_counts_in_its_wrap_width() {
    for scale in SCALES {
        set_device_scale(scale);
        // a hard break first, so the items land in the second, bordered, row
        let mut flow = FlowLeftRightWithWrapping::new();
        flow.row_margin = Insets::ZERO;
        flow.row_padding = Insets::ZERO;
        flow.row_border = Insets::symmetric(5.0, 0.0);
        flow.add_child(Box::new(HardBreak::new()));
        for _ in 0..4 {
            flow.add_child(item(50.0, 20.0));
        }

        let parent = in_parent(flow, 205.0);
        let flow = flow_of(&parent);
        let width = flow.bounds().width;

        // every item draws inside the flow
        let mut item_rows = Vec::new();
        for (r, row) in flow.children().iter().enumerate() {
            for (i, child) in row.children().iter().enumerate() {
                if child.type_name() == "Spacer" {
                    item_rows.push(r);
                    let right = drawn_right(flow, r, i);
                    // 200 units of items and a 10 unit border do not fit in 205
                    assert!(right <= width - 5.0 + 0.001, "{right} at {scale}x");
                }
            }
        }
        assert_eq!(item_rows.len(), 4);
        assert_ne!(item_rows[3], item_rows[0]);
    }
    set_device_scale(1.0);
}

/// The flow's own margin sits outside its width, so it takes nothing more from
/// the rows.
#[test]
fn the_flows_own_margin_does_not_narrow_its_rows() {
    for scale in SCALES {
        set_device_scale(scale);
        let flow = new_flow(4).with_margin(Insets::symmetric(10.0, 0.0));
        // 212 units of items and row chrome in a flow 212 units wide inside its margin
        let parent = in_parent(flow, 232.0);
        let flow = flow_of(&parent);

        assert!((flow.bounds().width - 212.0).abs() < 0.001, "at {scale}x");
        // the row fits the flow's own width exactly
        assert_eq!(filled_rows(flow), 1, "at {scale}x");
    }
    set_device_scale(1.0);
}

/// A right-aligned row ends its last item at the row's inner right edge.
#[test]
fn a_right_aligned_row_ends_at_its_inner_right_edge() {
    for scale in SCALES {
        set_device_scale(scale);
        let mut flow = FlowLeftRightWithWrapping::new();
        flow.set_content_h_anchor(HAnchor::RIGHT);
        flow.add_child(item(50.0, 20.0));
        flow.add_child(item(50.0, 20.0));

        let parent = in_parent(flow, 300.0);
        let flow = flow_of(&parent);
        let last = flow.children()[0].children().len() - 1;

        // 300 units less 3 of row margin and 3 of row padding
        assert!(
            (drawn_right(flow, 0, last) - 294.0).abs() < 0.001,
            "at {scale}x"
        );
    }
    set_device_scale(1.0);
}

/// Right alignment and Proportional spacing leave room for the flow's own
/// padding and a row's border, so a row ends at its inner right edge.
#[test]
fn aligned_rows_end_inside_the_flows_padding_and_the_rows_border() {
    for (scale, bordered, proportional) in [
        (1.0, false, false),
        (2.0, false, false),
        (1.0, true, false),
        (2.0, true, false),
        (1.0, false, true),
        (2.0, false, true),
    ] {
        set_device_scale(scale);
        let mut flow = FlowLeftRightWithWrapping::new();
        flow.padding = if bordered {
            Insets::ZERO
        } else {
            Insets::all(5.0)
        };
        flow.row_border = if bordered {
            Insets::symmetric(5.0, 0.0)
        } else {
            Insets::ZERO
        };
        flow.proportional = proportional;
        flow.set_content_h_anchor(if proportional {
            HAnchor::LEFT
        } else {
            HAnchor::RIGHT
        });
        if bordered {
            // a hard break first, so the items land in the second, bordered, row
            flow.add_child(Box::new(HardBreak::new()));
        }
        flow.add_child(item(50.0, 20.0));
        flow.add_child(item(50.0, 20.0));

        let parent = in_parent(flow, 300.0);
        let flow = flow_of(&parent);
        let row = flow.children().len() - 1;
        // proportional spacing ends each row with a spacer; right alignment ends
        // it with the last item - either way the row's last child
        let end = flow.children()[row].children().len() - 1;
        if !proportional {
            assert_eq!(flow.children()[row].children()[end].type_name(), "Spacer");
        }

        // 300 units less 5 of flow padding or row border, 3 of row margin and 3
        // of row padding
        let right = drawn_right(flow, row, end);
        assert!(
            (right - 289.0).abs() < 0.001,
            "{right} at {scale}x bordered {bordered} proportional {proportional}"
        );
    }
    set_device_scale(1.0);
}

/// The first item of every row is drawn one row margin plus one row padding in
/// from the flow's left edge.
#[test]
fn each_rows_first_item_is_drawn_one_padding_in() {
    for scale in SCALES {
        set_device_scale(scale);
        let parent = in_parent(new_flow(4), 160.0);
        let flow = flow_of(&parent);

        for (r, row) in flow.children().iter().enumerate() {
            if !row.children().is_empty() {
                // 3 units of row margin and 3 of row padding
                assert!((drawn_left(flow, r, 0) - 6.0).abs() < 0.001, "at {scale}x");
            }
        }
    }
    set_device_scale(1.0);
}

/// A flow narrowed each time it grows taller (a scroll bar appearing beside it)
/// re-wraps into more rows; its Fit height still ends enclosing every row, so
/// the bottom row is not clipped.
#[test]
fn a_flow_narrowed_while_it_grows_still_encloses_every_row() {
    for scale in SCALES {
        set_device_scale(scale);
        let mut flow = new_flow(12).with_h_anchor(HAnchor::LEFT);
        let mut width = 212.0;
        flow.set_width(width);
        let mut last_height = 0.0;
        // C#'s BoundsChanged handler: each growth takes 50 off while wider than 62.
        let mut size = flow.layout(Size::new(300.0, 500.0));
        while size.height > last_height && width > 62.0 {
            last_height = size.height;
            width -= 50.0;
            flow.set_width(width);
            size = flow.layout(Size::new(300.0, 500.0));
        }
        flow.set_bounds(agg_gui::Rect::new(0.0, 0.0, size.width, size.height));

        // each growth takes 50 off 212 until the width is 62
        assert_eq!(size.width, 62.0, "at {scale}x");
        // one 50 unit item and 12 units of row chrome fit in 62
        assert_eq!(filled_rows(&flow), 12, "at {scale}x");

        // a Fit flow is as tall as its rows
        let rows_top = flow
            .children()
            .iter()
            .map(|row| row.bounds().y + row.bounds().height)
            .fold(0.0, f64::max);
        let rows_bottom = flow
            .children()
            .iter()
            .map(|row| row.bounds().y)
            .fold(f64::MAX, f64::min);
        assert!(size.height >= rows_top - rows_bottom - 0.001, "at {scale}x");

        // the bottom row draws inside the flow
        let bottom_row = flow
            .children()
            .iter()
            .rfind(|row| !row.children().is_empty())
            .expect("a row");
        let bottom_item = bottom_row.children().last().expect("an item");
        let drawn_bottom = bottom_row.bounds().y + bottom_item.bounds().y;
        assert!(drawn_bottom >= -0.001, "at {scale}x");
    }
    set_device_scale(1.0);
}
