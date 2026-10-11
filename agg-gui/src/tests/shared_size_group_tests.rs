//! agg-sharp `Tests/Agg.Tests/Agg.UI/SharedSizeGroupTests.cs`, 1:1, plus the
//! `rust_only_*` tests pinning where agg-gui's shared size groups differ.
//!
//! C# `GuiWidget { Width, Height }` leaves are `SizedBox::fixed`; a
//! `FlowLayoutWidget` is a `FlexRow`, a top-to-bottom one a `FlexColumn`, and
//! a plain `GuiWidget` parent (`LayoutEngineSimpleAlign`) an
//! `AbsoluteLayout`. C# `PerformLayout` is `layout` at the scope's size; C#
//! re-lays a widget out when a child is added, agg-gui on the next layout,
//! so `DynamicAdd` lays out again after the push. `Visible = false` is a
//! hidden `Conditional`. Widths are logical units, unscaled in both.
//!
//! The rust-only tests pin the one divergence (see
//! `crate::shared_size_group`): a group narrows again when its widest member
//! is hidden, where C# keeps every member at the old widest width.

use std::cell::Cell;
use std::rc::Rc;

use crate::widgets::AbsoluteLayout;
use crate::{Conditional, FlexColumn, FlexRow, Size, SizedBox, Widget};

fn cell(w: f64, h: f64) -> SizedBox {
    SizedBox::fixed(w, h)
}

fn width_at(root: &dyn Widget, path: &[usize]) -> f64 {
    let mut node = root;
    for &i in path {
        node = node.children()[i].as_ref();
    }
    node.bounds().width
}

fn perform_layout(scope: &mut dyn Widget, w: f64, h: f64) {
    scope.layout(Size::new(w, h));
}

#[test]
fn basic_equalization_widths_match_largest() {
    let mut scope = FlexColumn::new()
        .with_shared_size_scope(true)
        .add(Box::new(
            FlexRow::new()
                .add(Box::new(cell(30.0, 20.0).with_shared_size_group("labels")))
                .add(Box::new(cell(100.0, 20.0))),
        ))
        .add(Box::new(
            FlexRow::new()
                .add(Box::new(cell(50.0, 20.0).with_shared_size_group("labels")))
                .add(Box::new(cell(100.0, 20.0))),
        ))
        .add(Box::new(
            FlexRow::new()
                .add(Box::new(cell(40.0, 20.0).with_shared_size_group("labels")))
                .add(Box::new(cell(100.0, 20.0))),
        ));

    perform_layout(&mut scope, 400.0, 200.0);

    assert_eq!(width_at(&scope, &[0, 0]), 50.0);
    assert_eq!(width_at(&scope, &[1, 0]), 50.0);
    assert_eq!(width_at(&scope, &[2, 0]), 50.0);
}

#[test]
fn multiple_groups_each_group_equalizes_independently() {
    let mut scope = FlexColumn::new()
        .with_shared_size_scope(true)
        .add(Box::new(
            FlexRow::new()
                .add(Box::new(cell(30.0, 20.0).with_shared_size_group("labels")))
                .add(Box::new(cell(80.0, 20.0).with_shared_size_group("values"))),
        ))
        .add(Box::new(
            FlexRow::new()
                .add(Box::new(cell(50.0, 20.0).with_shared_size_group("labels")))
                .add(Box::new(cell(60.0, 20.0).with_shared_size_group("values"))),
        ));

    perform_layout(&mut scope, 400.0, 200.0);

    // "labels" group: max(30, 50) = 50
    assert_eq!(width_at(&scope, &[0, 0]), 50.0);
    assert_eq!(width_at(&scope, &[1, 0]), 50.0);

    // "values" group: max(80, 60) = 80
    assert_eq!(width_at(&scope, &[0, 1]), 80.0);
    assert_eq!(width_at(&scope, &[1, 1]), 80.0);
}

#[test]
fn nested_scopes_groups_are_independent() {
    let mut outer_scope = FlexColumn::new()
        .with_shared_size_scope(true)
        .add(Box::new(cell(100.0, 20.0).with_shared_size_group("col")))
        .add(Box::new(
            FlexColumn::new()
                .with_shared_size_scope(true)
                .add(Box::new(cell(200.0, 20.0).with_shared_size_group("col"))),
        ));

    perform_layout(&mut outer_scope, 400.0, 300.0);

    // The outer "col" group should only contain outerLabel (innerLabel is in the nested scope)
    assert_eq!(width_at(&outer_scope, &[0]), 100.0);
    // The inner "col" group should only contain innerLabel
    assert_eq!(width_at(&outer_scope, &[1, 0]), 200.0);
}

#[test]
fn dynamic_add_new_widget_gets_equalized() {
    let mut scope = FlexColumn::new().with_shared_size_scope(true).add(Box::new(
        FlexRow::new().add(Box::new(cell(50.0, 20.0).with_shared_size_group("labels"))),
    ));

    perform_layout(&mut scope, 400.0, 200.0);
    assert_eq!(width_at(&scope, &[0, 0]), 50.0);

    // Add a new row with a wider label
    scope.push(
        Box::new(FlexRow::new().add(Box::new(cell(80.0, 20.0).with_shared_size_group("labels")))),
        0.0,
    );

    // After adding the new child, layout runs and equalizes
    perform_layout(&mut scope, 400.0, 200.0);
    assert_eq!(width_at(&scope, &[0, 0]), 80.0);
    assert_eq!(width_at(&scope, &[1, 0]), 80.0);
}

/// A hidden `Conditional` wrapping an 80-wide cell: C#'s
/// `new GuiWidget { Width = 80, SharedSizeGroupName = "labels", Visible = false }`.
fn hidden_member(visible: &Rc<Cell<bool>>, width: f64) -> Conditional {
    Conditional::new(Rc::clone(visible), Box::new(cell(width, 20.0)))
        .with_shared_size_group("labels")
}

#[test]
fn invisible_widgets_excluded_from_equalization() {
    let visible = Rc::new(Cell::new(false));
    let mut scope = FlexColumn::new()
        .with_shared_size_scope(true)
        .add(Box::new(FlexRow::new().add(Box::new(
            cell(30.0, 20.0).with_shared_size_group("labels"),
        ))))
        .add(Box::new(
            FlexRow::new().add(Box::new(hidden_member(&visible, 80.0))),
        ));

    perform_layout(&mut scope, 400.0, 200.0);

    // label2 is invisible, so label1 should stay at its own width
    assert_eq!(width_at(&scope, &[0, 0]), 30.0);
}

#[test]
fn no_scope_no_equalization() {
    // Container without IsSharedSizeScope - groups should not equalize
    let mut container = FlexColumn::new()
        .add(Box::new(cell(30.0, 20.0).with_shared_size_group("labels")))
        .add(Box::new(cell(50.0, 20.0).with_shared_size_group("labels")));

    perform_layout(&mut container, 400.0, 200.0);

    assert_eq!(width_at(&container, &[0]), 30.0);
    assert_eq!(width_at(&container, &[1]), 50.0);
}

#[test]
fn simple_align_layout_scope_works_without_flow() {
    let mut scope = AbsoluteLayout::new()
        .with_shared_size_scope(true)
        .add(Box::new(cell(30.0, 20.0).with_shared_size_group("col")))
        .add(Box::new(cell(50.0, 20.0).with_shared_size_group("col")));

    perform_layout(&mut scope, 400.0, 200.0);

    assert_eq!(width_at(&scope, &[0]), 50.0);
    assert_eq!(width_at(&scope, &[1]), 50.0);
}

#[test]
fn rust_only_hiding_the_widest_member_narrows_the_group_again() {
    let visible = Rc::new(Cell::new(true));
    let mut scope = FlexColumn::new()
        .with_shared_size_scope(true)
        .add(Box::new(FlexRow::new().add(Box::new(
            cell(30.0, 20.0).with_shared_size_group("labels"),
        ))))
        .add(Box::new(
            FlexRow::new().add(Box::new(hidden_member(&visible, 80.0))),
        ));

    perform_layout(&mut scope, 400.0, 200.0);
    assert_eq!(width_at(&scope, &[0, 0]), 80.0);

    // C# would leave label1 at 80: its raised MinimumSize is never lowered.
    visible.set(false);
    perform_layout(&mut scope, 400.0, 200.0);
    assert_eq!(width_at(&scope, &[0, 0]), 30.0);

    visible.set(true);
    perform_layout(&mut scope, 400.0, 200.0);
    assert_eq!(width_at(&scope, &[0, 0]), 80.0);
}

#[test]
fn rust_only_a_member_keeps_its_own_minimum_width_after_the_group_narrows() {
    let visible = Rc::new(Cell::new(true));
    let mut scope = FlexColumn::new()
        .with_shared_size_scope(true)
        .add(Box::new(
            FlexRow::new().add(Box::new(
                cell(30.0, 20.0)
                    .with_min_size(Size::new(40.0, 0.0))
                    .with_shared_size_group("labels"),
            )),
        ))
        .add(Box::new(
            FlexRow::new().add(Box::new(hidden_member(&visible, 80.0))),
        ));

    perform_layout(&mut scope, 400.0, 200.0);
    assert_eq!(width_at(&scope, &[0, 0]), 80.0);

    visible.set(false);
    perform_layout(&mut scope, 400.0, 200.0);
    assert_eq!(width_at(&scope, &[0, 0]), 40.0);
    let member = &scope.children()[0].children()[0];
    assert_eq!(member.min_size().width, 40.0);
}
