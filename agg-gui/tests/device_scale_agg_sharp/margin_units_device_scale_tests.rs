//! agg-sharp `MarginUnitsDeviceScaleTests`
//! (Tests/Agg.Tests/Agg.UI/MarginUnitsDeviceScaleTests.cs): margins and
//! padding are design units while bounds are device pixels, so every place
//! that mixes the two has to scale the design value. Each C# `[Arguments]` row
//! is one iteration of the test's scale loop.
//!
//! agg-gui keeps margins, padding and bounds all in logical units and scales
//! the painted tree once, so a logical measurement times the scale is the C#
//! device measurement.
//!
//! `IntegerBoundsRoundTheDevicePadding` is a behaviour test: C# rounds an
//! integer-bounds widget's device margin and padding to whole pixels;
//! agg-gui keeps both as given and instead snaps an integer-bounds widget's
//! paint translation to the device pixel grid (`enforce_integer_bounds`,
//! `paint_subtree`). What the user sees is the same, so the test pins that:
//! a widget placed 1.3 units of padding plus 1.3 of margin in paints with
//! whole-pixel edges, at 1x and at 2x.
//!
//! Not ported here: `SplitterDragStopsAtTheDevicePadding` tests agg-sharp's
//! distance `Splitter` with padding; agg-gui's ratio `Splitter` has neither
//! and MatterCAD does not use it (its splitters are mattercad-app's own).

use agg_gui::{Color, Container, FlexRow, Insets, Size, SizedBox, VAnchor, Widget};

use super::support::{at_device_scale, layout_at, paint_widget_scaled, pixel, WHITE};

const SCALES: [f64; 2] = [1.0, 2.0];
const RED: [u8; 4] = [255, 0, 0, 255];

/// A fractional padding on an integer-bounds widget lands its content on
/// whole device pixels, like its margin.
#[test]
fn integer_bounds_round_the_device_padding() {
    for scale in SCALES {
        at_device_scale(scale, || {
            let child = Container::new()
                .with_background(Color::rgba(1.0, 0.0, 0.0, 1.0))
                .with_inner_padding(Insets::ZERO)
                .with_min_size(Size::new(20.0, 20.0))
                .with_max_size(Size::new(20.0, 20.0))
                .with_margin(Insets::all(1.3));
            // C# sets `EnforceIntegerBounds = true`; it is agg-gui's default
            assert!(child.enforce_integer_bounds(), "widgets snap by default");
            let mut widget = FlexRow::new()
                .with_gap(0.0)
                .with_fit_width(true)
                .with_v_anchor(VAnchor::FIT)
                .with_inner_padding(Insets::all(1.3))
                .add(Box::new(child));
            layout_at(&mut widget, Size::new(100.0, 100.0));
            let fb = paint_widget_scaled(&mut widget);

            let mut red_columns = 0;
            let mut red_rows = 0;
            let middle_y = fb.height() / 2;
            let middle_x = fb.width() / 2;
            for x in 0..fb.width() {
                let p = pixel(&fb, x, middle_y);
                assert!(
                    p == WHITE || p == RED,
                    "column {x} at {scale}x is a part-covered edge: {p:?}"
                );
                red_columns += usize::from(p == RED);
            }
            for y in 0..fb.height() {
                let p = pixel(&fb, middle_x, y);
                assert!(
                    p == WHITE || p == RED,
                    "row {y} at {scale}x is a part-covered edge: {p:?}"
                );
                red_rows += usize::from(p == RED);
            }
            assert_eq!(red_columns as f64, 20.0 * scale, "at {scale}x");
            assert_eq!(red_rows as f64, 20.0 * scale, "at {scale}x");
        });
    }
}

/// The bounds of a widget's children, margins included, reach exactly each
/// child's margin past it.
#[test]
fn children_bounds_include_the_device_margin() {
    for scale in SCALES {
        at_device_scale(scale, || {
            let mut parent = FlexRow::new()
                .with_gap(0.0)
                .with_fit_width(true)
                .with_v_anchor(VAnchor::FIT)
                .with_inner_padding(Insets::ZERO)
                .add(Box::new(
                    SizedBox::fixed(50.0, 30.0).with_margin(Insets::all(5.0)),
                ));
            let bounds = layout_at(&mut parent, Size::new(400.0, 400.0));
            let width = bounds.width * scale;
            let height = bounds.height * scale;
            assert!(
                (width - (50.0 + 10.0) * scale).abs() <= 0.001,
                "the child plus its 5 design-unit margin each side at {scale}x: {width}"
            );
            assert!((height - (30.0 + 10.0) * scale).abs() <= 0.001, "{height}");
        });
    }
}
