//! agg-sharp `Tests/Agg.Tests/Agg.UI/SharedColumnWidthTests.cs`, 1:1: two
//! table rows whose cells share a group per column line their columns up.
//! Same mapping as `shared_size_group_tests.rs`: C# `GuiWidget { Width,
//! Height }` cells are `SizedBox::fixed`, `FlowLayoutWidget`s are
//! `FlexRow`/`FlexColumn`, `PerformLayout` is `layout` at the scope's size.

use crate::{FlexColumn, FlexRow, Size, SizedBox, Widget};

fn width_at(root: &dyn Widget, path: &[usize]) -> f64 {
    let mut node = root;
    for &i in path {
        node = node.children()[i].as_ref();
    }
    node.bounds().width
}

#[test]
fn shared_size_group_equalizes_column_widths() {
    let mut scope = FlexColumn::new()
        .with_shared_size_scope(true)
        .add(Box::new(
            FlexRow::new()
                .add(Box::new(
                    SizedBox::fixed(100.0, 20.0).with_shared_size_group("table_col_0"),
                ))
                .add(Box::new(
                    SizedBox::fixed(150.0, 20.0).with_shared_size_group("table_col_1"),
                )),
        ))
        .add(Box::new(
            FlexRow::new()
                .add(Box::new(
                    SizedBox::fixed(200.0, 20.0).with_shared_size_group("table_col_0"),
                ))
                .add(Box::new(
                    SizedBox::fixed(80.0, 20.0).with_shared_size_group("table_col_1"),
                )),
        ));

    scope.layout(Size::new(600.0, 200.0));

    // Column 0: max(100, 200) = 200
    assert_eq!(width_at(&scope, &[0, 0]), 200.0);
    assert_eq!(width_at(&scope, &[1, 0]), 200.0);

    // Column 1: max(150, 80) = 150
    assert_eq!(width_at(&scope, &[0, 1]), 150.0);
    assert_eq!(width_at(&scope, &[1, 1]), 150.0);
}
