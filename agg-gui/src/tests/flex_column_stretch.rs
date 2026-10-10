//! A `FlexColumn` child anchored to stretch vertically takes the free height,
//! as agg-sharp's top-to-bottom `FlowLayoutWidget` gives a `VAnchor.Stretch`
//! child its share (MatterCAD's `MatterCADUiFeatures` margin test relies on
//! it: a stretched check box with a 40-unit margin fills a 300-unit column to
//! 40 short of each end).

use crate::{FlexColumn, Insets, Rect, Size, SizedBox, VAnchor, Widget};

fn laid_out(mut column: FlexColumn, width: f64, height: f64) -> FlexColumn {
    column.layout(Size::new(width, height));
    column.set_bounds(Rect::new(0.0, 0.0, width, height));
    column
}

#[test]
fn a_vertically_stretched_child_fills_the_free_height_inside_its_margin() {
    let child = SizedBox::new()
        .with_height(20.0)
        .with_v_anchor(VAnchor::STRETCH)
        .with_margin(Insets::all(40.0));
    let column = laid_out(FlexColumn::new().add(Box::new(child)), 300.0, 300.0);
    let b = column.children()[0].bounds();
    assert_eq!((b.y, b.height), (40.0, 220.0), "{b:?}");
}

#[test]
fn stretched_children_share_the_free_height_beside_a_fixed_one() {
    let stretched = || {
        SizedBox::new()
            .with_height(10.0)
            .with_v_anchor(VAnchor::STRETCH)
    };
    let column = laid_out(
        FlexColumn::new()
            .with_gap(0.0)
            .add(Box::new(stretched()))
            .add(Box::new(SizedBox::new().with_height(100.0)))
            .add(Box::new(stretched())),
        100.0,
        300.0,
    );
    let heights: Vec<f64> = column
        .children()
        .iter()
        .map(|c| c.bounds().height)
        .collect();
    assert_eq!(heights, vec![100.0, 100.0, 100.0]);
}

#[test]
fn an_explicit_flex_factor_wins_over_the_anchor() {
    let column = laid_out(
        FlexColumn::new()
            .with_gap(0.0)
            .add_flex(
                Box::new(SizedBox::new().with_v_anchor(VAnchor::STRETCH)),
                3.0,
            )
            .add(Box::new(SizedBox::new().with_v_anchor(VAnchor::STRETCH))),
        100.0,
        400.0,
    );
    let heights: Vec<f64> = column
        .children()
        .iter()
        .map(|c| c.bounds().height)
        .collect();
    assert_eq!(heights, vec![300.0, 100.0]);
}
