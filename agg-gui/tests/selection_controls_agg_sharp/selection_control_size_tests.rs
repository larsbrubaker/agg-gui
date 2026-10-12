//! Port of agg-sharp `Tests/Agg.Tests/Agg.UI/SelectionControlSizeTests.cs`:
//! the classic check box slot and radio circle MatterCAD's layouts were built
//! around. C#'s classic size is `SelectionControlSize.BoxSize == null`;
//! agg-gui keeps egui's look as its default and makes the classic size the
//! opt-in (`selection_control_size.rs`), so the test selects it first.
//! `SetWidensTheCheckBoxSlotAndRadioCircle` (C#'s opt-in sized box) is not
//! ported: MatterCAD never sets a size.

use agg_gui::{Checkbox, RadioButton, Widget};

use super::support::{font, layout, use_classic_size, TWELVE_POINT};

#[test]
fn unset_keeps_the_classic_check_box_slot_and_radio_circle() {
    use_classic_size();
    assert_eq!(check_box_label_left(), 20.0);
    assert_eq!(radio_circle_width(), 11.0);
}

/// Where the label starts: the width of the slot the box is drawn in.
fn check_box_label_left() -> f64 {
    let mut view = Checkbox::new("Label", font(), false).with_font_size(TWELVE_POINT);
    layout(&mut view);
    view.children()[0].bounds().x
}

fn radio_circle_width() -> f64 {
    RadioButton::new("", font()).circle_slot_width()
}
