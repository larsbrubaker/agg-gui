//! A `ScrollView` found in a widget tree downcasts through `Widget::as_any`
//! / `as_any_mut`, so a test or an app reaches
//! [`ScrollView::scroll_bar_properties`] (the thumb and track rects) without
//! keeping its own handle to the view.

use crate::widget::{find_widget_by_type, walk_path_mut};
use crate::{
    Container, FlexColumn, ScrollBarVisibility, ScrollView, Size, SizedBox, VAnchor, Widget,
};

/// A container holding a column whose only row is a 200 x 100 scroll view
/// over content four times its height.
fn tree() -> Container {
    let content = SizedBox::new().with_width(180.0).with_height(400.0);
    let scroll = ScrollView::new(Box::new(content))
        .with_bar_visibility(ScrollBarVisibility::AlwaysVisible)
        .with_v_anchor(VAnchor::STRETCH);
    let column = FlexColumn::new()
        .with_gap(0.0)
        .add_flex(Box::new(scroll), 1.0);
    let mut root = Container::new().add(Box::new(column));
    root.layout(Size::new(200.0, 100.0));
    root
}

#[test]
fn a_scroll_view_in_a_tree_downcasts_to_read_its_thumb() {
    let root = tree();
    let found = find_widget_by_type(&root, "ScrollView").expect("a ScrollView in the tree");
    let scroll = found
        .as_any()
        .and_then(|any| any.downcast_ref::<ScrollView>())
        .expect("ScrollView downcasts through as_any");
    let bars = scroll.scroll_bar_properties();
    let thumb = bars.vertical.thumb;
    let track = bars.vertical.track;
    assert!(bars.vertical.showing, "content overflows, so the bar shows");
    assert!(thumb.width > 0.0 && thumb.height > 0.0, "thumb {thumb:?}");
    // A quarter of the content is in view, so the thumb is shorter than its
    // track and lies inside it, at the top (nothing scrolled yet).
    assert!(
        thumb.height < track.height,
        "thumb {thumb:?} track {track:?}"
    );
    assert!(thumb.y >= track.y - 1e-9 && thumb.y + thumb.height <= track.y + track.height + 1e-9);
    assert!((thumb.y + thumb.height - (track.y + track.height)).abs() < 1e-6);
}

#[test]
fn a_scroll_view_in_a_tree_downcasts_mutably() {
    let mut root = tree();
    let scroll = walk_path_mut(&mut root, &[0, 0])
        .and_then(|w| w.as_any_mut())
        .and_then(|any| any.downcast_mut::<ScrollView>())
        .expect("ScrollView downcasts through as_any_mut");
    assert!(scroll.scroll_bar_properties().vertical.content > 300.0);
}
