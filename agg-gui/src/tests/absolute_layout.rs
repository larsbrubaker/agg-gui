//! `AbsoluteLayout`: children sit at their own origin (C# `new Button(x, y)`,
//! `OriginRelativeParent`), and anchored axes follow C#'s
//! `LayoutEngineSimpleAlign`.

use crate::widgets::AbsoluteLayout;
use crate::{HAnchor, Insets, Point, Rect, Size, SizedBox, VAnchor, Widget};

fn sized(w: f64, h: f64) -> SizedBox {
    SizedBox::new().with_width(w).with_height(h)
}

fn laid_out(layout: AbsoluteLayout, available: Size) -> (Size, Vec<Rect>) {
    let mut layout = layout;
    let size = layout.layout(available);
    let rects = layout.children().iter().map(|c| c.bounds()).collect();
    (size, rects)
}

#[test]
fn children_sit_at_their_origin() {
    let layout = AbsoluteLayout::new()
        .add(Box::new(sized(50.0, 20.0).with_origin(10.0, 40.0)))
        .add(Box::new(sized(30.0, 10.0).with_origin(110.0, 40.0)));
    let (size, rects) = laid_out(layout, Size::new(200.0, 200.0));
    assert_eq!(size, Size::new(200.0, 200.0));
    assert_eq!(rects[0], Rect::new(10.0, 40.0, 50.0, 20.0));
    assert_eq!(rects[1], Rect::new(110.0, 40.0, 30.0, 10.0));
}

#[test]
fn a_child_without_an_origin_sits_at_zero() {
    let (_, rects) = laid_out(
        AbsoluteLayout::new().add(Box::new(sized(5.0, 6.0))),
        Size::new(100.0, 100.0),
    );
    assert_eq!(rects[0], Rect::new(0.0, 0.0, 5.0, 6.0));
}

#[test]
fn integer_bounds_round_the_origin_like_csharp() {
    // C# rounds OriginRelativeParent with Math.Round (banker's) when
    // EnforceIntegerBounds is on.
    let (_, rects) = laid_out(
        AbsoluteLayout::new().add(Box::new(sized(5.0, 5.0).with_origin(10.5, 11.5))),
        Size::new(100.0, 100.0),
    );
    assert_eq!((rects[0].x, rects[0].y), (10.0, 12.0));
}

#[test]
fn stretch_fills_the_padded_area_minus_margins() {
    let child = sized(5.0, 5.0)
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::STRETCH)
        .with_margin(Insets {
            left: 3.0,
            right: 4.0,
            top: 5.0,
            bottom: 6.0,
        })
        .with_origin(70.0, 70.0);
    let layout = AbsoluteLayout::new()
        .with_padding(Insets::all(10.0))
        .add(Box::new(child));
    let (_, rects) = laid_out(layout, Size::new(200.0, 100.0));
    assert_eq!(
        rects[0],
        Rect::new(13.0, 16.0, 200.0 - 20.0 - 7.0, 100.0 - 20.0 - 11.0)
    );
}

#[test]
fn left_right_top_bottom_and_center_anchor_the_child() {
    let layout = AbsoluteLayout::new()
        .add(Box::new(
            sized(20.0, 10.0)
                .with_h_anchor(HAnchor::RIGHT)
                .with_v_anchor(VAnchor::TOP)
                .with_origin(5.0, 5.0),
        ))
        .add(Box::new(
            sized(20.0, 10.0)
                .with_h_anchor(HAnchor::CENTER)
                .with_v_anchor(VAnchor::CENTER),
        ))
        .add(Box::new(
            sized(20.0, 10.0)
                .with_h_anchor(HAnchor::LEFT)
                .with_v_anchor(VAnchor::BOTTOM)
                .with_origin(50.0, 50.0),
        ));
    let (_, rects) = laid_out(layout, Size::new(100.0, 60.0));
    assert_eq!(rects[0], Rect::new(80.0, 50.0, 20.0, 10.0));
    assert_eq!(rects[1], Rect::new(40.0, 25.0, 20.0, 10.0));
    assert_eq!(rects[2], Rect::new(0.0, 0.0, 20.0, 10.0));
}

#[test]
fn an_anchor_on_one_axis_keeps_the_origin_on_the_other() {
    let (_, rects) = laid_out(
        AbsoluteLayout::new().add(Box::new(
            sized(20.0, 10.0)
                .with_h_anchor(HAnchor::RIGHT)
                .with_origin(7.0, 33.0),
        )),
        Size::new(100.0, 100.0),
    );
    assert_eq!(rects[0], Rect::new(80.0, 33.0, 20.0, 10.0));
}

#[test]
fn half_anchors_split_the_parent() {
    let (_, rects) = laid_out(
        AbsoluteLayout::new()
            .add(Box::new(
                sized(1.0, 1.0).with_h_anchor(HAnchor::LEFT | HAnchor::CENTER),
            ))
            .add(Box::new(
                sized(1.0, 1.0).with_h_anchor(HAnchor::CENTER | HAnchor::RIGHT),
            )),
        Size::new(100.0, 40.0),
    );
    assert_eq!((rects[0].x, rects[0].width), (0.0, 50.0));
    assert_eq!((rects[1].x, rects[1].width), (50.0, 50.0));
}

#[test]
fn a_fit_layout_encloses_its_children() {
    let layout = AbsoluteLayout::new()
        .with_h_anchor(HAnchor::FIT)
        .with_v_anchor(VAnchor::FIT)
        .add(Box::new(sized(50.0, 20.0).with_origin(10.0, 40.0)))
        .add(Box::new(sized(30.0, 10.0).with_origin(110.0, 5.0)));
    let (size, _) = laid_out(layout, Size::new(500.0, 500.0));
    assert_eq!(size, Size::new(140.0, 60.0));
}

#[test]
fn hidden_children_are_not_placed() {
    let mut layout = AbsoluteLayout::new()
        .with_h_anchor(HAnchor::FIT)
        .with_v_anchor(VAnchor::FIT)
        .add(Box::new(sized(10.0, 10.0)))
        .add(Box::new(crate::widgets::Conditional::new(
            std::rc::Rc::new(std::cell::Cell::new(false)),
            Box::new(sized(90.0, 90.0)),
        )));
    assert_eq!(
        layout.layout(Size::new(300.0, 300.0)),
        Size::new(10.0, 10.0)
    );
}

#[test]
fn origin_is_stored_on_the_widget_base() {
    let b = sized(1.0, 1.0).with_origin(3.0, 4.0);
    assert_eq!(
        b.widget_base().map(|w| w.origin),
        Some(Point::new(3.0, 4.0))
    );
}
