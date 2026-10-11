//! Port of agg-sharp `FlowLayoutPaddingTests.cs`: a flow (agg-gui
//! `FlexRow`/`FlexColumn`, C# `FlowLayoutWidget`) places its first item one
//! padding in from its drawn edge, whatever order its padding and stretch
//! anchor were set in.
//!
//! C# runs each case at device scale 1 and 2 and asserts device pixels
//! (`5 * scale`); agg-gui lays out in logical units and applies the device
//! scale once at the App's paint boundary, so the same cases assert the
//! logical value at both scales. The builders have no order-dependent state,
//! so the padding-first and stretch-first cases build the same flow in the
//! two orders.
//!
//! Skipped: `ABottomToTopFlowPaddedBeforeStretchDrawsItsFirstItemOnePaddingIn`
//! — agg-gui has no bottom-to-top flow, and MatterCAD never builds a
//! `FlowDirection.BottomToTop` flow (only agg-sharp's image_filters example
//! does), so the test covers code MatterCAD does not use.

use agg_gui::widgets::AbsoluteLayout;
use agg_gui::{FlexColumn, FlexRow, HAnchor, Insets, Label, Size};

use super::support::{bounds_at, font, layout_at, sized, ScaleGuard};

#[test]
fn a_left_to_right_flow_padded_before_stretch_draws_its_first_item_one_padding_in() {
    for scale in [1.0, 2.0] {
        let _scale = ScaleGuard::set(scale);
        let flow = FlexRow::new()
            .with_gap(0.0)
            .with_inner_padding(Insets::all(5.0))
            .with_h_anchor(HAnchor::STRETCH)
            .add(Box::new(sized(20.0, 20.0)));
        let mut parent = AbsoluteLayout::new().add(Box::new(flow));
        layout_at(&mut parent, Size::new(200.0, 100.0));

        // child bounds are relative to the flow's drawn bottom-left corner
        let item_left = bounds_at(&parent, &[0, 0]).x;
        assert!(
            (item_left - 5.0).abs() <= 0.001,
            "one 5 unit padding in from the flow's drawn left edge, at {scale}x: {item_left}"
        );
    }
}

fn padded_column(padding: Insets, h_anchor: HAnchor, padding_first: bool) -> FlexColumn {
    if padding_first {
        FlexColumn::new()
            .with_gap(0.0)
            .with_inner_padding(padding)
            .with_h_anchor(h_anchor)
    } else {
        FlexColumn::new()
            .with_gap(0.0)
            .with_h_anchor(h_anchor)
            .with_inner_padding(padding)
    }
}

#[test]
fn a_top_to_bottom_flow_draws_unaligned_children_one_padding_in_whichever_of_padding_and_stretch_comes_first(
) {
    for (scale, padding_first) in [(1.0, true), (1.0, false), (2.0, true), (2.0, false)] {
        let _scale = ScaleGuard::set(scale);
        let flow = padded_column(Insets::all(10.0), HAnchor::STRETCH, padding_first)
            .add(Box::new(sized(20.0, 20.0)))
            .add(Box::new(Label::new("label", font())))
            .add(Box::new(sized(20.0, 20.0).with_h_anchor(HAnchor::STRETCH)));
        let mut parent = AbsoluteLayout::new().add(Box::new(flow));
        layout_at(&mut parent, Size::new(200.0, 100.0));

        let flow_bounds = bounds_at(&parent, &[0]);
        assert!(
            flow_bounds.x.abs() <= 0.001,
            "the flow still fills its parent"
        );
        assert!((flow_bounds.width - 200.0).abs() <= 0.001);
        for i in 0..3 {
            let child_left = bounds_at(&parent, &[0, i]).x;
            assert!(
                (child_left - 10.0).abs() <= 0.001,
                "child {i} sits one 10 unit padding in, padding first: {padding_first}, at {scale}x: {child_left}"
            );
        }
    }
}

#[test]
fn a_fixed_width_top_to_bottom_flow_given_left_padding_keeps_its_bounds_and_origin_and_insets_its_children(
) {
    // MatterCAD's VerticalResizeContainer: an Absolute vertical flow narrower than its content, given a left
    // padding the width of its splitter.
    for (scale, padding_first) in [(1.0, true), (1.0, false), (2.0, true), (2.0, false)] {
        let _scale = ScaleGuard::set(scale);
        let flow = padded_column(
            Insets::from_sides(6.0, 0.0, 0.0, 0.0),
            HAnchor::ABSOLUTE,
            padding_first,
        )
        // C# `flow.Width = 375` on an Absolute flow: a fixed width
        .with_min_size(Size::new(375.0, 0.0))
        .with_max_size(Size::new(375.0, f64::MAX))
        .add(Box::new(sized(20.0, 20.0).with_h_anchor(HAnchor::STRETCH)))
        .add(Box::new(sized(500.0, 20.0)));
        let mut parent = AbsoluteLayout::new().add(Box::new(flow));
        layout_at(&mut parent, Size::new(800.0, 400.0));

        let flow_bounds = bounds_at(&parent, &[0]);
        // padding insets the children, not the flow's own bounds; the flow
        // does not move when given padding
        assert!(
            flow_bounds.x.abs() <= 0.001,
            "at {scale}x: {}",
            flow_bounds.x
        );
        assert!((flow_bounds.width - 375.0).abs() <= 0.001);
        assert!((bounds_at(&parent, &[0, 0]).width - 369.0).abs() <= 0.001);
        for i in 0..2 {
            let child_left = bounds_at(&parent, &[0, i]).x;
            assert!(
                (child_left - 6.0).abs() <= 0.001,
                "child {i} sits one 6 unit padding in, padding first: {padding_first}, at {scale}x: {child_left}"
            );
        }
    }
}
