//! A `ComboBox`'s own hover help (C# `DropDownList.ToolTipText`): set with
//! the `Widget` tooltip setters every `WidgetBase` widget shares
//! ([`Widget::with_tooltip`], [`Widget::set_tooltip_text`]), read by the
//! central tooltip controller when the pointer rests on the closed box.

use super::*;
use crate::tests::keyboard_lift_harness::{ScaleGuard, SystemFontGuard, TooltipGuard};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::tooltip::{controller, tooltip_timings};
use crate::ComboBox;
use std::sync::Arc;
use std::time::Duration;

const VP: Size = Size {
    width: 300.0,
    height: 200.0,
};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

fn combo() -> ComboBox {
    ComboBox::new(vec!["Millimeters", "Inches"], 0, font())
}

#[test]
fn combo_box_takes_a_tooltip_through_the_widget_setters() {
    let tipped = combo().with_tooltip("Choose the unit lengths are shown in");
    assert_eq!(
        tipped.tooltip_text(),
        Some("Choose the unit lengths are shown in")
    );

    let mut boxed: Box<dyn Widget> = Box::new(combo());
    assert_eq!(boxed.tooltip_text(), None);
    boxed.set_tooltip_text(Some("Pick one".to_string()));
    assert_eq!(boxed.tooltip_text(), Some("Pick one"));
    boxed.set_tooltip_text(None);
    assert_eq!(boxed.tooltip_text(), None);
}

#[test]
fn combo_box_tooltip_shows_when_the_pointer_rests_on_it() {
    let _g = (
        ScaleGuard::set(1.0, 1.0),
        TooltipGuard::new(),
        SystemFontGuard::set(font()),
    );
    let mut app = App::new(Box::new(combo().with_tooltip("Pick one")));
    app.layout(VP);

    // The combo sits at the root's origin; rest the pointer inside it.
    app.on_mouse_move(10.0, VP.height - 5.0);
    let mut ctx = super::paint_recorder::PaintRecorder::new();
    app.paint(&mut ctx);
    crate::clock::advance(tooltip_timings().initial_delay + Duration::from_millis(10));
    let mut ctx = super::paint_recorder::PaintRecorder::new();
    app.paint(&mut ctx);

    assert_eq!(controller::visible_text().as_deref(), Some("Pick one"));
}
