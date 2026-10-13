//! Rust-only tests of the names an open caret suggestion list answers to in
//! a widget search (agg-gui `widgets/text_suggestion/names.rs`, agg-sharp
//! `TextSuggestionPopup.Name`, `RowName` and its `FindDescendants`): the
//! runner's standard flow finds a row by name, clicks it and waits for
//! "Text Suggestions" to disappear, as MatterCAD's
//! `ExpressionAutocompleteAutomationTests` does, and a row scrolled out of
//! view is not found until the highlight brings it into view. No C# test
//! covers the popup's names on their own; the C# controller tests are
//! ported in agg-gui's `text_suggestion/controller_tests.rs`.

use std::sync::Arc;

use agg_gui::widgets::{TextField, TextSuggestion, TextSuggestionController, TextSuggestionList};
use agg_gui::{Rect, Widget};
use agg_gui_automation::tree_query::find_by_name;
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationRunner, AutomationWindow, RunOptions, WaitOpts,
};

const COLORS: [&str; 3] = ["[red]", "[green]", "[blue]"];

/// Twelve words starting "c", more than the list shows at once (eight).
const MANY: [&str; 12] = [
    "c00", "c01", "c02", "c03", "c04", "c05", "c06", "c07", "c08", "c09", "c10", "c11",
];

/// After a '[' the bracket names starting with what follows it (replacing
/// from the '['); otherwise the `MANY` words starting with the word before
/// the caret.
fn provider(text: &str, caret: usize) -> TextSuggestionList {
    let before = &text[..caret];
    if let Some(open) = before.rfind('[') {
        let typed = &before[open..];
        let matches = COLORS
            .iter()
            .filter(|c| c.starts_with(typed))
            .map(|c| TextSuggestion::new(*c))
            .collect();
        return TextSuggestionList::new(open, caret - open, matches);
    }
    let start = before
        .rfind(|c: char| !c.is_alphanumeric())
        .map_or(0, |i| i + 1);
    let word = &before[start..];
    if word.is_empty() {
        return TextSuggestionList::empty();
    }
    let matches = MANY
        .iter()
        .filter(|w| w.starts_with(word))
        .map(|w| TextSuggestion::new(*w))
        .collect();
    TextSuggestionList::new(start, caret - start, matches)
}

/// A 400 × 300 window holding the field "field" near its top, offering the
/// provider's suggestions; the list hangs below it.
fn run(body: impl FnOnce(&mut AutomationRunner) + Send + 'static) {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let mut window = AutomationWindow::new(400.0, 300.0);
            let mut field = TextField::new(Arc::new(agg_gui::fonts::standard_ui_font()))
                .with_text_suggestions(TextSuggestionController::new(provider))
                .with_name("field");
            field.set_bounds(Rect::new(20.0, 250.0, 300.0, 24.0));
            window.add_child(Box::new(field));
            (window, ())
        },
        |runner, _| {
            body(runner);
            runner.mark_test_complete();
        },
    )
    .expect("the test body passes");
}

fn field(runner: &AutomationRunner) -> &TextField {
    let root = runner.app().root();
    find_by_name(root, "field")
        .first()
        .and_then(|handle| handle.downcast::<TextField>(root))
        .expect("the field is in the window")
}

fn field_has_focus(runner: &AutomationRunner) -> bool {
    let root = runner.app().root();
    let path = find_by_name(root, "field")
        .first()
        .and_then(|handle| handle.resolve(root))
        .expect("the field is in the window");
    runner.app().focused_path() == Some(path.as_slice())
}

#[test]
fn rust_only_a_suggestion_row_is_clicked_by_name_and_the_list_disappears() {
    run(|runner| {
        assert!(
            !runner.name_exists(TextSuggestionController::POPUP_NAME, 0.0, true),
            "a closed list is not found"
        );
        runner.click_by_name("field").type_text("=[r");

        assert!(runner.wait_for_name(TextSuggestionController::POPUP_NAME, 2.0));
        let red_row = TextSuggestionController::row_name("[red]");
        assert_eq!(red_row, "[red] Suggestion");
        assert!(
            runner.wait_for_name(&red_row, 2.0),
            "typing \"=[r\" has to list [red]"
        );
        assert!(
            !runner.name_exists(&TextSuggestionController::row_name("[green]"), 0.0, true),
            "a row the list does not show is not found"
        );

        let hit = runner
            .get_object_by_name(&red_row, &WaitOpts::default())
            .expect("the row is on show");
        let suggestion = hit
            .target
            .as_ref()
            .and_then(|t| t.object_as::<TextSuggestion>())
            .expect("a row carries its suggestion");
        assert_eq!(suggestion.label, "[red]");

        runner.click_by_name(&red_row);
        assert_eq!(
            field(runner).text(),
            "=[red]",
            "a click on the row has to put the name in"
        );
        assert!(
            field_has_focus(runner),
            "the click on the list must not take the focus off the field"
        );

        runner.type_text(" - 40");
        assert!(
            runner.wait_for_widget_disappear(TextSuggestionController::POPUP_NAME, 5.0),
            "nothing is offered after a number, so the list has to be closed"
        );
        assert!(!runner.name_exists(&red_row, 0.0, true));
        assert_eq!(field(runner).text(), "=[red] - 40");

        // Escape closes an open list, and its name goes with it.
        runner.type_text(" [g");
        assert!(runner.wait_for_name(&TextSuggestionController::row_name("[green]"), 2.0));
        runner.type_text("{Esc}");
        assert!(runner.wait_for_widget_disappear(TextSuggestionController::POPUP_NAME, 5.0));
        assert_eq!(field(runner).text(), "=[red] - 40 [g");
    });
}

#[test]
fn rust_only_a_suggestion_row_scrolled_out_of_view_is_not_found_until_it_is_in_view() {
    run(|runner| {
        runner.click_by_name("field").type_text("c");
        let first = TextSuggestionController::row_name("c00");
        let last_in_view = TextSuggestionController::row_name("c07");
        let out_of_view = TextSuggestionController::row_name("c10");
        assert!(runner.wait_for_name(&first, 2.0));
        assert!(runner.name_exists(&last_in_view, 0.0, true));
        assert!(
            !runner.name_exists(&out_of_view, 0.0, true),
            "only the eight rows in view are reported"
        );
        assert!(
            !runner.name_exists(&out_of_view, 0.0, false),
            "a row out of view is not reported at all, as in C#"
        );

        // The highlight brings row 10 into view, scrolling row 0 out.
        for _ in 0..10 {
            runner.type_text("{Down}");
        }
        assert!(runner.wait_for_name(&out_of_view, 2.0));
        assert!(!runner.name_exists(&first, 0.0, false));

        runner.click_by_name(&out_of_view);
        assert_eq!(field(runner).text(), "c10");
    });
}
