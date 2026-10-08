//! Port of agg-sharp `Tests/Agg.Tests/Agg Automation Tests/TextEditFocusTests.cs`.
//!
//! C#'s single-line `TextEditWidget(pixelWidth: 200)` centred in a 300 × 200
//! `SystemWindow` is an agg-gui `TextField`, 200 wide at its own height,
//! placed at the window's centre (the window's root keeps children where
//! they are put, so the centring C#'s anchors do is done here). C#'s
//! `editField.Focus()` posted with `UiThread.RunOnIdle` is a
//! `focus::request_focus` posted the same way, with the field built under
//! that focus id; `editField.ContainsFocus` is the `App`'s focused path
//! starting at the field; `editField.Text` and `SelectAllOnFocus` are read
//! and set on the field found by name. `BackgroundColor` is not ported: the
//! window colour does not bear on focus or typing.

use std::sync::Arc;

use agg_gui::widgets::TextField;
use agg_gui::{focus, ui_thread, Rect, Size, Widget};
use agg_gui_automation::tree_query::find_by_name;
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationRunner, AutomationWindow, RunOptions,
};

const EDIT_FIELD: &str = "editField";
const EDIT_FIELD_FOCUS_ID: focus::FocusId = 1;

#[test]
fn verify_focus_makes_text_widget_editable() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || (centered_window(edit_field()), ()),
        |test_runner, _| {
            // Focus on the ui thread, and wait until it has landed: calling Focus from the test thread
            // races the window's own first frames, which can move focus after it.
            ui_thread::run_on_idle(|| focus::request_focus(EDIT_FIELD_FOCUS_ID));
            test_runner.wait_for(contains_focus, 5.0, 10);

            // Type returns once the ui thread has delivered every key.
            test_runner.type_text("Test Text");

            assert!(text(test_runner) == "Test Text");
            test_runner.mark_test_complete();
        },
    )
    .expect("a focused text field takes typing");
}

#[test]
fn verify_focus_property() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || (centered_window(edit_field()), ()),
        |test_runner, _| {
            ui_thread::run_on_idle(|| focus::request_focus(EDIT_FIELD_FOCUS_ID));
            test_runner.wait_for(contains_focus, 5.0, 10);
            // NOTE: Okay. During parallel testing, it seems that the avalanche of windows causes test UIs to lose control focus and get confused.
            assert!(contains_focus(test_runner));
            test_runner.mark_test_complete();
        },
    )
    .expect("focusing a text field gives it focus");
}

#[test]
fn select_all_on_focus_can_still_click_after_selection() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || (centered_window(edit_field().with_text("Some Text")), ()),
        |test_runner, _| {
            // Set on the ui thread, and let the window settle before clicking rather than sleeping.
            set_select_all_on_focus(test_runner);
            test_runner.wait_for_pending_ui_work(250);
            test_runner.click_by_name(EDIT_FIELD);
            test_runner.wait_for(contains_focus, 5.0, 10);

            test_runner.type_text("123");
            // Text input on newly focused control should replace selection
            assert_eq!(text(test_runner), "123");

            test_runner.click_by_name(EDIT_FIELD);
            test_runner.wait_for(contains_focus, 5.0, 10);

            test_runner.type_text("123");
            // Text should be appended if control is focused and has already received input
            assert_eq!(text(test_runner), "123123");
            test_runner.mark_test_complete();
        },
    )
    .expect("select-all-on-focus replaces on the first click and appends after the second");
}

/// C# `new TextEditWidget(pixelWidth: 200) { Name = "editField" }`.
fn edit_field() -> TextField {
    TextField::new(Arc::new(agg_gui::fonts::standard_ui_font()))
        .with_name(EDIT_FIELD)
        .with_focus_id(EDIT_FIELD_FOCUS_ID)
}

/// C#'s `new SystemWindow(300, 200)` holding `field` with
/// `HAnchor.Center, VAnchor.Center`: 200 wide, its own height, centred.
fn centered_window(mut field: TextField) -> AutomationWindow {
    let (width, height) = (300.0, 200.0);
    let field_width = 200.0;
    let field_height = field.layout(Size::new(field_width, height)).height;
    field.set_bounds(Rect::new(
        (width - field_width) / 2.0,
        (height - field_height) / 2.0,
        field_width,
        field_height,
    ));
    let mut window = AutomationWindow::new(width, height);
    window.add_child(Box::new(field));
    window
}

/// C# `editField.ContainsFocus`: focus is on the field or inside it.
fn contains_focus(test_runner: &AutomationRunner) -> bool {
    let app = test_runner.app();
    let Some(field) = find_by_name(app.root(), EDIT_FIELD).into_iter().next() else {
        return false;
    };
    match (field.resolve(app.root()), app.focused_path()) {
        (Some(path), Some(focus)) => focus.starts_with(&path),
        _ => false,
    }
}

/// C# `editField.Text`.
fn text(test_runner: &AutomationRunner) -> String {
    let root = test_runner.app().root();
    find_by_name(root, EDIT_FIELD)
        .first()
        .and_then(|handle| handle.downcast::<TextField>(root))
        .map(TextField::text)
        .expect("the edit field is in the window")
}

/// C# `editField.SelectAllOnFocus = true`, set on the UI thread (the body
/// is the UI thread).
fn set_select_all_on_focus(test_runner: &mut AutomationRunner) {
    let root = test_runner.driver_mut().root_mut();
    let handle = find_by_name(root, EDIT_FIELD)
        .into_iter()
        .next()
        .expect("the edit field is in the window");
    let field = handle
        .widget_mut(root)
        .and_then(|widget| widget.as_any_mut())
        .and_then(|any| any.downcast_mut::<TextField>())
        .expect("the edit field is a TextField");
    field.set_select_all_on_focus(true);
}
