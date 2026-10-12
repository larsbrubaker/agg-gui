//! Port of agg-sharp's `TreeViewLateContentClippingTests`
//! (Tests/Agg.Tests/Agg.UI/TreeViewLateContentClippingTests.cs).
//!
//! A library tree fills in asynchronously: nodes are handed to a `TreeView`
//! that is already on screen, and its rows first exist on the frame after.
//! In C# those rows painted in the window's corner, outside the scroll area,
//! on the one frame they appeared.  agg-gui builds rows in `layout`, which the
//! frame runs before paint; this pins that every row painted on that frame
//! stays inside the scroll area.
//!
//! `DirtyFlagAndAwaitingCountStayInStepUnderConcurrentUpdates` is skipped:
//! agg-gui's `TreeView` has no cross-thread dirty flag or awaiting count.

use std::sync::Arc;

use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, VAnchor};
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::tree_view::{NodeIcon, TreeView};
use crate::widgets::AbsoluteLayout;

use super::paint_recorder::{DeviceRect, PaintRecorder};

/// A 400 x 600 window with a 300 x 400 dialog inset at (50, 60), the dialog
/// holding a stretched `TreeView` (the C# harness's toolbar strip only made the
/// escape visible; the bounds checks below don't need it).
fn window() -> AbsoluteLayout {
    let tree = TreeView::new(Arc::new(Font::from_slice(super::TEST_FONT).expect("font")))
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::STRETCH);
    let dialog = AbsoluteLayout::new()
        .with_h_anchor(HAnchor::ABSOLUTE)
        .with_v_anchor(VAnchor::ABSOLUTE)
        .with_min_size(Size::new(300.0, 400.0))
        .with_max_size(Size::new(300.0, 400.0))
        .with_origin(50.0, 60.0)
        .add(Box::new(tree));
    AbsoluteLayout::new().add(Box::new(dialog))
}

fn tree_of(root: &mut AbsoluteLayout) -> &mut TreeView {
    root.children_mut()[0].children_mut()[0]
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<TreeView>())
        .expect("the tree")
}

/// One frame: layout, then paint, recording every text with its clip.
fn draw(root: &mut AbsoluteLayout) -> PaintRecorder {
    root.set_bounds(Rect::new(0.0, 0.0, 400.0, 600.0));
    root.layout(Size::new(400.0, 600.0));
    let mut rec = PaintRecorder::new();
    paint_subtree(root, &mut rec);
    rec
}

#[test]
fn rows_added_between_frames_stay_inside_the_scroll_area_on_the_frame_they_appear() {
    let mut root = window();

    // Frame one: the dialog is up and the tree is empty.
    draw(&mut root);

    // The library content arrives between frames.
    let tree = tree_of(&mut root);
    let parts = tree.add_root("Parts", NodeIcon::Folder);
    for i in 0..40 {
        tree.add_child(parts, format!("Part {i:02}"), NodeIcon::File);
    }
    tree.expand(parts);

    // Frame two: the frame the user saw the glitch on.
    let rec = draw(&mut root);
    let viewport = (50.0, 60.0, 350.0, 460.0);
    // Each row paints its name as text or as its label's cached bitmap.
    let rows: Vec<(String, DeviceRect, Option<DeviceRect>)> = rec
        .text_clips
        .iter()
        .filter(|(t, _, _)| t.starts_with("Part"))
        .map(|(t, (x, y), clip)| (t.clone(), (*x, *y, *x, *y), *clip))
        .chain(
            rec.image_clips
                .iter()
                .map(|(dst, clip)| ("image".to_string(), *dst, *clip)),
        )
        .collect();
    assert!(
        rows.len() > 5,
        "if the rows never painted, the assertions below prove nothing"
    );

    for (name, at, clip) in rows {
        let clip = clip.unwrap_or_else(|| panic!("'{name}' at {at:?} painted with no clip"));
        assert!(
            clip.0 >= viewport.0 - 1.0,
            "'{name}' may paint left of the scroll area ({clip:?})"
        );
        assert!(
            clip.2 <= viewport.2 + 1.0,
            "'{name}' may paint right of the scroll area ({clip:?})"
        );
        assert!(
            clip.3 <= viewport.3 + 1.0,
            "'{name}' may paint above the scroll area ({clip:?})"
        );
        assert!(
            clip.1 >= viewport.1 - 1.0,
            "'{name}' may paint below the scroll area ({clip:?})"
        );
        // Where a visible row landed has to be in the viewport too.
        let visible = at.0 < clip.2 && at.2 > clip.0 && at.1 < clip.3 && at.3 > clip.1;
        if visible {
            assert!(
                at.1 <= viewport.3 + 1.0,
                "'{name}' painted above the scroll area at {at:?}"
            );
        }
    }
}
