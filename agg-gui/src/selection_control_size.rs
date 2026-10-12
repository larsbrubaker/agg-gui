//! The size of the box a [`Checkbox`](crate::widgets::Checkbox) draws and of
//! a [`RadioButton`](crate::widgets::RadioButton)'s circle (agg-sharp
//! `Gui/Theme/SelectionControlSize.cs`).
//!
//! agg-gui's own look ([`SelectionControlSize::Egui`], the default) is egui's:
//! a 16 box after a 2 focus pad with an 8 gap before the label, and a 14
//! circle. agg-sharp's controls instead keep the classic geometry
//! MatterCAD's layouts were built around ([`SelectionControlSize::Classic`]):
//! a 12 box at (2, round((h - 12) / 2)) in a 20 wide slot that ends where the
//! label starts, and a 10 circle in an 11 square followed by a 10 gap. An app
//! that wants the classic size sets it once at startup, before it builds any
//! controls; like C#, each control reads the size when it is constructed.
//!
//! C#'s `SelectionControlSize.BoxSize` is null for the classic size; agg-gui
//! keeps egui's look as its default so its other consumers (and its egui
//! reference demo) are unchanged, and the classic size is the opt-in. The
//! setting is thread local, like [`device_scale`](crate::device_scale).

use std::cell::Cell;

/// Which geometry new check boxes and radio buttons use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionControlSize {
    /// agg-gui's (egui's) 16 box and 14 circle.
    #[default]
    Egui,
    /// agg-sharp's classic 12 box in a 20 slot and 10 circle in an 11 square.
    Classic,
}

thread_local! {
    static SIZE: Cell<SelectionControlSize> = const { Cell::new(SelectionControlSize::Egui) };
}

/// The geometry check boxes and radio buttons built now will use.
pub fn selection_control_size() -> SelectionControlSize {
    SIZE.with(Cell::get)
}

/// Set the geometry for check boxes and radio buttons built from now on.
pub fn set_selection_control_size(size: SelectionControlSize) {
    SIZE.with(|s| s.set(size));
}

/// The focus pad left of the box or circle, in logical units.
pub(crate) const LEADING_PAD: f64 = 2.0;

/// Classic check box: `CheckBoxViewText.CheckBoxWidth` (10); the slot is
/// twice it and the box 1.2 times it.
pub(crate) const CLASSIC_CHECK_BOX_WIDTH: f64 = 10.0;

/// Classic radio: `RadioImage.BoxWidth`, the circle's diameter.
pub(crate) const CLASSIC_RADIO_BOX_WIDTH: f64 = 10.0;

/// Classic radio: `RadioCircleWidget`'s right margin before the label.
pub(crate) const CLASSIC_RADIO_LABEL_GAP: f64 = 10.0;
