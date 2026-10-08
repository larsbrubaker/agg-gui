//! Port of agg-sharp `Tests/Agg.Tests/Agg Automation Tests/MouseInteractionTests.cs`.
//!
//! Only the tests whose subjects have landed are here; the rest of the class
//! (clicks, enter/leave, capture) arrives with the runner slices listed in
//! `docs/design/gui-automation.md`. C#'s bare `GuiWidget`s are
//! [`ProbeWidget`]s, and C# widget references are [`WidgetHandle`]s, whose
//! equality is widget identity.

use agg_gui::Widget;
use agg_gui_automation::tree_query::{children, parents};
use agg_gui_automation::{ProbeWidget, WidgetHandle};

#[test]
fn extension_methods_tests() {
    let level3 = ProbeWidget::new("level3");
    let level2 = ProbeWidget::new("level2").with_child(Box::new(level3));
    let level1 = ProbeWidget::new("level1").with_child(Box::new(level2));
    let level0 = ProbeWidget::new("level0").with_child(Box::new(level1));
    let root: &dyn Widget = &level0;
    let handle = |path: &[usize]| WidgetHandle::new(root, path).expect("widget at path");
    let all_widgets = [
        handle(&[]),
        handle(&[0]),
        handle(&[0, 0]),
        handle(&[0, 0, 0]),
    ];

    for child in children(root, &all_widgets[0]) {
        assert!(child == all_widgets[1]);
    }

    for child in children(root, &all_widgets[1]) {
        assert!(child == all_widgets[2]);
    }

    for child in children(root, &all_widgets[2]) {
        assert!(child == all_widgets[3]);
    }

    // C# loops over the children and throws inside the loop body. A loop
    // whose body always panics trips clippy's `never_loop` deny, so the same
    // check looks at the first child instead: any child at all fails.
    if let Some(_child) = children(root, &all_widgets[3]).first() {
        panic!("there are no children we should not get here");
    }

    let mut index = all_widgets.len() - 1;
    let mut parent_count = 0;
    for parent in parents(root, &all_widgets[3]) {
        parent_count += 1;
        index -= 1;
        assert!(parent == all_widgets[index]);
    }

    assert!(parent_count == 3);
}
