//! Rust-only tests of the automation file dialog (agg-gui-automation
//! `src/dialog/`, C# `AutomationDialogProvider`): C#'s reading of the typed
//! text, the dialog shown through an `AutomationWindow`'s built-in overlay
//! layer, answered by name with `complete_dialog` (MatterCAD's
//! `CompleteDialog`), cancelled with Escape, and focused when it shows.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::widgets::TextField;
use agg_gui_automation::pointer_state;
use agg_gui_automation::{
    parse_dialog_text, show_window_and_execute_tests, AutomationError, AutomationFileDialog,
    AutomationModalPresenter, AutomationRunner, AutomationWindow, FileDialogKind, FileDialogResult,
    ModalPresenter, RunOptions, WaitOpts, AUTOMATION_DIALOG_TEXT_EDIT,
};

type Answer = Rc<RefCell<Option<FileDialogResult>>>;

/// Run `body` against an 800 × 400 window (room for C#'s 600 × 200 dialog)
/// whose presenter and answer slot are handed through.
fn run_with_dialog<R: Send + 'static>(
    body: impl FnOnce(&mut AutomationRunner, &(AutomationModalPresenter, Answer)) -> R + Send + 'static,
) -> Result<R, AutomationError> {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let window = AutomationWindow::new(800.0, 400.0);
            let presenter = window.modal_presenter();
            (window, (presenter, Answer::default()))
        },
        move |runner, state| {
            let result = body(runner, state);
            runner.mark_test_complete();
            result
        },
    )
}

/// Show an open-file dialog whose answer lands in `answer`.
fn show(presenter: &dyn ModalPresenter, kind: FileDialogKind, answer: &Answer) {
    let answer = Rc::clone(answer);
    AutomationFileDialog::show(presenter, kind, move |result| {
        *answer.borrow_mut() = Some(result);
    });
}

fn answered(runner: &mut AutomationRunner, answer: &Answer) -> Option<FileDialogResult> {
    runner.wait_for(|_| answer.borrow().is_some(), 5.0, 10);
    answer.borrow().clone()
}

#[test]
fn rust_only_dialog_text_splits_on_semicolons_and_trims_quotes_like_csharp() {
    assert_eq!(
        parse_dialog_text("\"/a/b.stl\";\"/c/d.stl\""),
        Some(vec!["/a/b.stl".to_string(), "/c/d.stl".to_string()])
    );
    assert_eq!(parse_dialog_text("abc"), Some(vec!["abc".to_string()]));
    // C# trims only the quotes, never the spaces between paths.
    assert_eq!(
        parse_dialog_text("\"a\"; \"b\""),
        Some(vec!["a".to_string(), " \"b".to_string()])
    );
    // Two characters or fewer is "cancelled", counted in UTF-16 units.
    assert_eq!(parse_dialog_text("ab"), None);
    assert_eq!(parse_dialog_text(""), None);
    assert_eq!(parse_dialog_text("\u{1F600}"), None);
    assert!(FileDialogResult::from_text(FileDialogKind::Save, "ab").is_cancelled());
}

#[test]
fn rust_only_completing_the_dialog_by_name_hands_the_typed_paths_to_the_callback() {
    let result = run_with_dialog(|runner, (presenter, answer)| {
        show(presenter, FileDialogKind::Open, answer);
        runner.complete_dialog("\"/parts/a.stl\";\"/parts/b.stl\"");
        let gone = !runner.name_exists(AUTOMATION_DIALOG_TEXT_EDIT, 0.0, true);
        (answered(runner, answer), gone)
    })
    .expect("the run succeeds");
    assert_eq!(
        result,
        (
            Some(FileDialogResult {
                kind: FileDialogKind::Open,
                file_name: Some("/parts/a.stl".to_string()),
                file_names: vec!["/parts/a.stl".to_string(), "/parts/b.stl".to_string()],
            }),
            true
        )
    );
}

#[test]
fn rust_only_the_dialog_field_holds_focus_when_it_shows() {
    let focused = run_with_dialog(|runner, (presenter, answer)| {
        show(presenter, FileDialogKind::Save, answer);
        assert!(runner.wait_for_name(AUTOMATION_DIALOG_TEXT_EDIT, 2.0));
        let field = runner
            .get_widget_by_name(AUTOMATION_DIALOG_TEXT_EDIT, &WaitOpts::default())
            .expect("the field shows");
        pointer_state::focused(runner.app(), &field)
    })
    .expect("the run succeeds");
    assert!(focused);
}

#[test]
fn rust_only_escape_closes_the_dialog_with_nothing_chosen() {
    let result = run_with_dialog(|runner, (presenter, answer)| {
        show(presenter, FileDialogKind::Save, answer);
        assert!(runner.wait_for_name(AUTOMATION_DIALOG_TEXT_EDIT, 2.0));
        runner.type_text("/never/used.stl").type_text("{ESC}");
        let gone = runner.wait_for_widget_disappear(AUTOMATION_DIALOG_TEXT_EDIT, 2.0);
        (answered(runner, answer), gone)
    })
    .expect("the run succeeds");
    assert_eq!(
        result,
        (
            Some(FileDialogResult::cancelled(FileDialogKind::Save)),
            true
        )
    );
}

#[test]
fn rust_only_two_characters_or_fewer_are_a_cancelled_dialog() {
    let result = run_with_dialog(|runner, (presenter, answer)| {
        show(presenter, FileDialogKind::Open, answer);
        runner.complete_dialog("ab");
        answered(runner, answer)
    })
    .expect("the run succeeds");
    assert_eq!(
        result,
        Some(FileDialogResult::cancelled(FileDialogKind::Open))
    );
}

#[test]
fn rust_only_the_callback_runs_on_idle_after_the_dialog_has_closed() {
    // C# posts the callback with RunOnIdle: by the time it runs, the dialog
    // is out of the tree.
    let field_at_callback = run_with_dialog(|runner, (presenter, _)| {
        let seen: Rc<RefCell<Option<bool>>> = Rc::default();
        let record = Rc::clone(&seen);
        AutomationFileDialog::show(presenter, FileDialogKind::Open, move |_| {
            *record.borrow_mut() = Some(true);
        });
        runner.complete_dialog("/parts/a.stl");
        runner.wait_for(|_| seen.borrow().is_some(), 5.0, 10);
        let ran = seen.borrow().is_some();
        (
            ran,
            runner.name_exists(AUTOMATION_DIALOG_TEXT_EDIT, 0.0, true),
        )
    })
    .expect("the run succeeds");
    assert_eq!(field_at_callback, (true, false));
}

#[test]
fn rust_only_a_dialog_presented_on_the_window_while_it_is_built_shows_at_load() {
    let typed = show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let window = AutomationWindow::new(800.0, 400.0);
            let answer = Answer::default();
            show(&window, FileDialogKind::Open, &answer);
            (window, answer)
        },
        |runner, answer| {
            runner.complete_dialog("/parts/at_load.stl");
            let result = answered(runner, answer);
            runner.mark_test_complete();
            result.and_then(|r| r.file_name)
        },
    )
    .expect("the run succeeds");
    assert_eq!(typed.as_deref(), Some("/parts/at_load.stl"));
}

#[test]
fn rust_only_the_dialog_field_is_a_text_field() {
    let is_field = run_with_dialog(|runner, (presenter, answer)| {
        show(presenter, FileDialogKind::Open, answer);
        assert!(runner.wait_for_name(AUTOMATION_DIALOG_TEXT_EDIT, 2.0));
        let field = runner
            .get_widget_by_name(AUTOMATION_DIALOG_TEXT_EDIT, &WaitOpts::default())
            .expect("the field shows");
        field.downcast::<TextField>(runner.app().root()).is_some()
    })
    .expect("the run succeeds");
    assert!(is_field);
}

#[test]
fn rust_only_a_window_that_never_asks_for_a_presenter_keeps_its_root() {
    let root_type = show_window_and_execute_tests(
        RunOptions::default(),
        || (AutomationWindow::new(300.0, 200.0), ()),
        |runner, _| {
            runner.mark_test_complete();
            runner.app().root().type_name()
        },
    )
    .expect("the run succeeds");
    assert_ne!(root_type, "Stack");
}

#[test]
fn rust_only_completing_a_dialog_that_never_shows_fails_with_csharps_message() {
    let error = run_with_dialog(|runner, _| {
        runner.complete_dialog("/parts/a.stl");
    })
    .expect_err("there is no dialog to complete");
    assert!(
        error.to_string().contains(
            "Required widget 'Automation Dialog TextEdit' was not found within 5 seconds."
        ),
        "{error}"
    );
}
