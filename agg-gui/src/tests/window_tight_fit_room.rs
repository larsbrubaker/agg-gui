//! A window that hugs its content (`with_tight_content_fit`) never grows
//! past the room between its top and the bottom of its canvas: on a short
//! screen (a phone) its content's ScrollView scrolls instead of the window
//! running off the bottom where nothing can reach it.

use std::sync::Arc;

use crate::text::Font;
use crate::widgets::{ScrollView, SizedBox, Window};
use crate::{Rect, Size, Widget};

use super::TEST_FONT;

#[test]
fn a_content_hugging_window_stops_at_the_bottom_of_the_screen_and_its_content_scrolls() {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let content = Box::new(ScrollView::new(Box::new(
        SizedBox::new().with_height(1200.0),
    )));
    let mut win = Window::new("Tall", font, content)
        .with_bounds(Rect::new(10.0, 100.0, 300.0, 300.0))
        .with_tight_content_fit(true);
    win.layout(Size::new(400.0, 600.0));
    let b = win.bounds();
    assert_eq!(b.y + b.height, 400.0, "the window's top stays put");
    assert!(b.y >= 0.0, "the window ends on screen: bottom at {}", b.y);
    let scroll = win.children()[0].bounds();
    assert!(
        scroll.height < 1200.0,
        "the content scrolls in {} px",
        scroll.height
    );
    // A short content still hugs.
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let content = Box::new(ScrollView::new(Box::new(
        SizedBox::new().with_height(120.0),
    )));
    let mut win = Window::new("Short", font, content)
        .with_bounds(Rect::new(10.0, 100.0, 300.0, 300.0))
        .with_tight_content_fit(true);
    win.layout(Size::new(400.0, 600.0));
    assert!(win.bounds().height < 200.0, "{}", win.bounds().height);
}
