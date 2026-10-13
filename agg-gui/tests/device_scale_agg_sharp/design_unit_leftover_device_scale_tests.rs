//! agg-sharp `DesignUnitLeftoverDeviceScaleTests`
//! (Tests/Agg.Tests/Agg.UI/DesignUnitLeftoverDeviceScaleTests.cs): padding and
//! margins are design units, so a text's padding and a centred flow row keep
//! their design geometry at 1x and 2x. Each C# `[Arguments]` row is one
//! iteration of the test's scale loop.
//!
//! C# measures device pixels; agg-gui lays out in logical units and the App
//! scales the whole tree by the device scale when it paints, so a logical
//! measurement times the scale is the C# device measurement, and the C#
//! expectations are compared in those device pixels.
//!
//! `AutoExpandingTextIsTheTextPlusItsPadding`: an agg-gui `Label` carries no
//! padding; a text gets padding from the fit row it sits in (C#'s
//! `AutoExpandBoundsToText` text widget is a fit `FlowLayoutWidget` here).
//!
//! Not ported here:
//! - `WrappingHappensAtTheSameDesignWidth` duplicates
//!   `flow_left_right_with_wrapping_tests::resizing_the_parent_rewraps_at_the_new_width`
//!   (the same four 50 unit items wrap at 206 and fit at 212, at 1x and 2x).
//! - `NumberEditStartsTwoUnitsPastItsLabel` (MatterCAD's `ThemedNumberEdit`)
//!   and `SplitterPanelSitsInsideItsBorder` (agg-sharp's distance `Splitter`
//!   with a bordered `Panel1`) test widgets agg-gui does not have; MatterCAD's
//!   are ported in mattercad-app (its number edit and its own splitters), so
//!   these belong with those.

use agg_gui::{
    FlexColumn, FlexRow, FlowLeftRightWithWrapping, HAnchor, Insets, Label, Size, SizedBox,
    VAnchor, Widget,
};

use super::support::{at_device_scale, font, layout_at};

const SCALES: [f64; 2] = [1.0, 2.0];

/// An auto-expanding text is its text plus its padding, in device pixels.
#[test]
fn auto_expanding_text_is_the_text_plus_its_padding() {
    for scale in SCALES {
        at_device_scale(scale, || {
            let mut alone = Label::new("Padded", font()).with_font_size(12.0);
            let text_bounds = alone.layout(Size::new(1000.0, 1000.0));

            let mut text = FlexRow::new()
                .with_gap(0.0)
                .with_fit_width(true)
                .with_inner_padding(Insets::all(5.0))
                .with_v_anchor(VAnchor::FIT)
                .add(Box::new(
                    Label::new("Padded", font())
                        .with_font_size(12.0)
                        .with_enforce_integer_bounds(false),
                ));
            let size = layout_at(&mut text, Size::new(1000.0, 1000.0));

            let width = size.width * scale;
            let height = size.height * scale;
            assert!(
                (width - (text_bounds.width * scale + 10.0 * scale)).abs() <= 0.001,
                "the text plus 5 design units of padding each side at {scale}x: {width}"
            );
            assert!(
                (height - (text_bounds.height * scale + 10.0 * scale)).abs() <= 0.001,
                "at {scale}x: {height}"
            );
        });
    }
}

/// A centred wrapping flow puts its row's content in the middle of its width.
#[test]
fn a_centred_row_is_in_the_middle() {
    for scale in SCALES {
        at_device_scale(scale, || {
            let mut flow = FlowLeftRightWithWrapping::new();
            flow.center = true;
            flow.add_child(Box::new(SizedBox::fixed(50.0, 20.0)));
            flow.add_child(Box::new(SizedBox::fixed(50.0, 20.0)));

            let mut parent = FlexColumn::new()
                .with_inner_padding(Insets::ZERO)
                .with_h_anchor(HAnchor::STRETCH);
            parent.push(Box::new(flow), 0.0);
            layout_at(&mut parent, Size::new(300.0, 500.0));

            // where the item draws in the flow: its row's place plus its place
            // in the row (the row also holds the spacer that centres it)
            let flow = parent.children()[0].as_ref();
            let row = &flow.children()[0];
            let first = row
                .children()
                .iter()
                .find(|c| c.type_name() == "SizedBox")
                .expect("the first item");
            let first_left = (row.bounds().x + first.bounds().x) * scale;
            assert!(
                (first_left - 100.0 * scale).abs() <= 0.001,
                "100 units of items centred in 300 start 100 units in, at {scale}x: {first_left}"
            );
        });
    }
}
