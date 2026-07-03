//! Keyboard-avoidance lift must track FRESH layout bounds.
//!
//! The focus-change hook computes the lift before the tree re-lays out,
//! so a field revealed in the same event (search panel opening) reports
//! stale zero bounds and produces a garbage lift that nothing used to
//! correct — the whole UI stayed shifted up with the panel's top edge
//! clipped off-screen (the mobile "search panel cut off" bug).
//! `App::layout` now re-evaluates the lift every frame while the
//! on-screen keyboard is up.

use super::*;
use crate::text::Font;
use crate::widget::keyboard_scroll;
use std::sync::Arc;

/// A focused field that comfortably clears the keyboard must pull any
/// stale lift back to zero on the next layout.
#[test]
fn layout_corrects_stale_keyboard_lift() {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    keyboard_scroll::reset_lift_for_test();
    crate::widgets::on_screen_keyboard::set_enabled(true);

    const FIELD_ID: u64 = 777;
    // Field pinned near the top of a tall viewport — far above any
    // keyboard panel.
    let root = FlexColumn::new().add(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(14.0)
            .with_focus_id(FIELD_ID),
    ));

    let mut app = App::new(Box::new(root));
    app.layout(Size::new(400.0, 800.0));
    crate::focus::request_focus(FIELD_ID);
    app.layout(Size::new(400.0, 800.0));
    assert_eq!(
        app.focused_widget_type_name(),
        Some("TextField"),
        "precondition: the field took focus (keyboard visible)"
    );

    // Simulate the focus-instant miscomputation: a lift far larger than
    // anything the layout justifies.
    keyboard_scroll::request_lift(500.0);
    app.layout(Size::new(400.0, 800.0));

    assert_eq!(
        keyboard_scroll::lift_target_for_test(),
        0.0,
        "layout must re-evaluate the lift against fresh bounds and \
         cancel the stale shift"
    );

    crate::widgets::on_screen_keyboard::set_enabled(false);
    keyboard_scroll::reset_lift_for_test();
}
