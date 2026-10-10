//! `ResponsiveImageWidget` (a port of agg-sharp `Gui/ResponsiveImageWidget.cs`,
//! which has no tests of its own): the picture fills the width it is given up
//! to its own width, keeps its aspect ratio, and a picture the host fills in
//! later resizes the widget at the next layout.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::{ResponsiveImageWidget, Size, Widget};

fn picture(width: u32, height: u32) -> Option<(Vec<u8>, u32, u32)> {
    Some((vec![255; (width * height * 4) as usize], width, height))
}

#[test]
fn a_narrow_slot_scales_the_picture_down_keeping_its_aspect() {
    let image = Rc::new(RefCell::new(picture(200, 50)));
    let mut widget = ResponsiveImageWidget::new(image);

    assert_eq!(
        widget.layout(Size::new(100.0, 500.0)),
        Size::new(100.0, 25.0)
    );
}

#[test]
fn a_wide_slot_shows_the_picture_at_its_own_size() {
    let image = Rc::new(RefCell::new(picture(200, 50)));
    let mut widget = ResponsiveImageWidget::new(image);

    assert_eq!(widget.layout(Size::new(800.0, 500.0)).height, 50.0);
    assert_eq!(widget.max_size(), Size::new(200.0, 50.0));
}

#[test]
fn an_explicit_max_width_caps_the_scale() {
    let image = Rc::new(RefCell::new(picture(200, 50)));
    let mut widget = ResponsiveImageWidget::new(image).with_max_width(100.0);

    assert_eq!(widget.layout(Size::new(800.0, 500.0)).height, 25.0);
}

#[test]
fn a_picture_that_arrives_later_resizes_the_widget() {
    let image = Rc::new(RefCell::new(None));
    let mut widget = ResponsiveImageWidget::new(Rc::clone(&image));
    assert_eq!(widget.layout(Size::new(400.0, 500.0)).height, 0.0);

    *image.borrow_mut() = picture(1520, 170);

    assert_eq!(widget.layout(Size::new(760.0, 500.0)).height, 85.0);
}
