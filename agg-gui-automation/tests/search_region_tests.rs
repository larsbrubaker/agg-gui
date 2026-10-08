//! Rust-only tests for `ScreenRectangle` / `SearchRegion` (C# has no test
//! class of its own for them; the runner tests exercise them indirectly).

use std::cell::Cell;

use agg_gui::Framebuffer;
use agg_gui_automation::{ScreenRectangle, SearchRegion};

#[test]
fn rust_only_intersection_clips_to_the_overlap() {
    let a = ScreenRectangle::new(0, 0, 100, 50);
    let b = ScreenRectangle::new(40, 10, 200, 30);
    assert_eq!(
        ScreenRectangle::intersection(a, b),
        Some(ScreenRectangle::new(40, 10, 100, 30))
    );
}

#[test]
fn rust_only_rectangles_that_only_touch_do_not_intersect() {
    let a = ScreenRectangle::new(0, 0, 10, 10);
    let b = ScreenRectangle::new(10, 0, 20, 10);
    assert_eq!(ScreenRectangle::intersection(a, b), None);
}

#[test]
fn rust_only_the_region_image_is_captured_once_on_first_use() {
    let region = SearchRegion::new(ScreenRectangle::new(0, 0, 4, 4));
    assert!(region.captured_image().is_none());
    let captures = Cell::new(0);
    let capture = || {
        captures.set(captures.get() + 1);
        Framebuffer::new(4, 4)
    };
    assert_eq!(region.image(capture).width(), 4);
    assert_eq!(region.image(|| Framebuffer::new(9, 9)).width(), 4);
    assert_eq!(captures.get(), 1);
}

#[test]
fn rust_only_a_region_given_an_image_never_captures() {
    let region = SearchRegion::with_image(Framebuffer::new(3, 2), ScreenRectangle::new(0, 0, 3, 2));
    let image = region.image(|| panic!("an image was supplied"));
    assert_eq!((image.width(), image.height()), (3, 2));
}
