//! ReserveInset → overlay_insets integration: wrapping edge chrome in
//! `ReserveInset` must publish the strip it occupies so anchored overlay
//! placement (`card::anchored_rect`) can avoid it — the "info card hides
//! under the left button rail on mobile" bug class.

use crate::layout_props::{HAnchor, VAnchor};
use crate::widgets::{ReserveInset, Stack};
use crate::{overlay_insets, Size, SizedBox, Widget};

#[test]
fn stack_child_wrapped_in_reserve_inset_publishes_its_strip() {
    overlay_insets::begin_frame();
    crate::widget::set_current_viewport(Size::new(400.0, 700.0));

    // A 56-wide left rail and a 90-tall bottom tray, both anchored the
    // way app chrome typically is.
    let rail = Box::new(
        SizedBox::fixed(56.0, 300.0)
            .with_h_anchor(HAnchor::LEFT)
            .with_v_anchor(VAnchor::CENTER),
    );
    let tray = Box::new(
        SizedBox::fixed(400.0, 90.0)
            .with_h_anchor(HAnchor::CENTER)
            .with_v_anchor(VAnchor::BOTTOM),
    );
    let mut stack = Stack::new()
        .add_aligned(Box::new(ReserveInset::left(rail)))
        .add_aligned(Box::new(ReserveInset::bottom(tray)));
    stack.layout(Size::new(400.0, 700.0));

    let r = overlay_insets::current();
    assert_eq!(r.left, 56.0, "rail strip reserved");
    assert_eq!(r.bottom, 90.0, "tray strip reserved");
    assert_eq!(r.right, 0.0);

    // The reservation is frame-scoped: the next layout pass starts clean.
    overlay_insets::begin_frame();
    assert_eq!(overlay_insets::current().left, 0.0);
}
