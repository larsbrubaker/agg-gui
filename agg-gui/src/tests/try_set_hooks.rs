//! The inspector edits a widget through `Widget::try_set_padding` /
//! `try_set_visible`. A wrapping flow and its rows report and take their
//! padding (C# `Padding`), the flow wrapping again with the new padding in its
//! rows' chrome width; a `ReserveInset` forwards visibility to its child,
//! whose visibility it reports.

use std::cell::Cell;
use std::rc::Rc;

use crate::geometry::Size;
use crate::layout_props::Insets;
use crate::widget::Widget;
use crate::widgets::{Conditional, FlowLeftRightWithWrapping, ReserveInset, SizedBox};

fn flow_of_boxes() -> FlowLeftRightWithWrapping {
    let mut flow = FlowLeftRightWithWrapping::new();
    for _ in 0..4 {
        flow.add_child(Box::new(SizedBox::fixed(40.0, 10.0)));
    }
    flow
}

#[test]
fn rust_only_flow_reports_and_takes_its_padding() {
    let mut flow = flow_of_boxes();
    assert_eq!(flow.padding(), Insets::ZERO);
    let pad = Insets::all(7.0);
    assert!(flow.try_set_padding(pad));
    assert_eq!(flow.padding(), pad);
    assert_eq!(flow.padding, pad);
}

#[test]
fn rust_only_flow_padding_edit_wraps_again() {
    // 4 boxes of 40 plus the rows' chrome (margin 3+3, padding 3+3) fit one
    // row at 200 px. Padding the flow by 30 a side leaves 128 for items, so
    // they wrap to two rows on the next layout at the same width.
    let mut flow = flow_of_boxes();
    flow.layout(Size::new(200.0, 400.0));
    assert_eq!(flow.children().len(), 1);
    assert!(flow.try_set_padding(Insets::symmetric(30.0, 0.0)));
    flow.layout(Size::new(200.0, 400.0));
    assert_eq!(flow.children().len(), 2);
}

#[test]
fn rust_only_wrap_row_reports_and_takes_its_padding() {
    let mut flow = flow_of_boxes();
    flow.layout(Size::new(200.0, 400.0));
    let row = &mut flow.children_mut()[0];
    assert_eq!(row.padding(), Insets::all(3.0));
    let before = row.layout(Size::new(200.0, 400.0)).height;
    assert!(row.try_set_padding(Insets::all(10.0)));
    assert_eq!(row.padding(), Insets::all(10.0));
    let after = row.layout(Size::new(200.0, 400.0)).height;
    assert_eq!(after - before, 14.0, "7 more on top and on bottom");
    assert_eq!(row.children()[0].bounds().x, 10.0);
}

#[test]
fn rust_only_reserve_inset_sets_its_childs_visibility() {
    let shown = Rc::new(Cell::new(true));
    let child = Conditional::new(Rc::clone(&shown), Box::new(SizedBox::fixed(10.0, 10.0)));
    let mut inset = ReserveInset::left(Box::new(child));
    assert!(inset.is_visible());
    assert!(inset.try_set_visible(false));
    assert!(!shown.get());
    assert!(!inset.is_visible());
    assert!(inset.try_set_visible(true));
    assert!(inset.is_visible());
}

#[test]
fn rust_only_reserve_inset_reports_a_child_without_visibility() {
    // A child with no visibility of its own refuses, and so does the inset.
    let mut inset = ReserveInset::left(Box::new(SizedBox::fixed(10.0, 10.0)));
    assert!(!inset.try_set_visible(false));
    assert!(inset.is_visible());
}
