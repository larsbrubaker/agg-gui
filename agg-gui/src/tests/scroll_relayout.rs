//! `ScrollView` measures its content at an effectively unbounded height
//! (`f64::MAX / 2`) and then gives it bounds of the measured height.  Content
//! that places its children from the TOP of the height it is laid out in — a
//! `FlexColumn::with_top_anchor(true)` or a vertical `MenuBar` — left its
//! children up near 1e308 unless the scroll view laid it out again at the
//! measured height.  These pin every direct child inside the content box.

use std::sync::Arc;

use crate::text::Font;
use crate::widgets::menu::{MenuBar, MenuOrientation, TopMenu};
use crate::widgets::Label;
use crate::{FlexColumn, ScrollView, Size, Widget};

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Every direct child of the scroll view's content lies inside the content.
fn assert_children_inside_content(sv: &ScrollView) {
    let content = &sv.children()[0];
    let h = content.bounds().height;
    assert!(h > 0.0 && h < 1.0e6, "content height {h}");
    for (i, child) in content.children().iter().enumerate() {
        let b = child.bounds();
        assert!(
            b.y >= -0.5 && b.y + b.height <= h + 0.5,
            "child {i} at {b:?} lies outside the {h}-tall content"
        );
    }
}

#[test]
fn top_anchored_column_in_a_scroll_view_keeps_its_children_in_view() {
    let font = test_font();
    let column = FlexColumn::new()
        .with_top_anchor(true)
        .add(Box::new(Label::new("One", Arc::clone(&font))))
        .add(Box::new(Label::new("Two", Arc::clone(&font))))
        .add(Box::new(Label::new("Three", Arc::clone(&font))));
    let mut sv = ScrollView::new(Box::new(column));
    sv.layout(Size::new(200.0, 30.0));
    assert_children_inside_content(&sv);
    // The first child sits at the top of the content.
    let content = &sv.children()[0];
    let first = content.children()[0].bounds();
    assert!((first.y + first.height - content.bounds().height).abs() < 0.5);
}

#[test]
fn vertical_menu_bar_in_a_scroll_view_keeps_its_titles_in_view() {
    let bar = MenuBar::new(
        test_font(),
        vec![
            TopMenu::new("File", vec![]),
            TopMenu::new("Edit", vec![]),
            TopMenu::new("View", vec![]),
        ],
        |_| {},
    )
    .with_orientation(MenuOrientation::Vertical);
    let mut sv = ScrollView::new(Box::new(bar));
    sv.layout(Size::new(120.0, 40.0));
    assert_children_inside_content(&sv);
}
