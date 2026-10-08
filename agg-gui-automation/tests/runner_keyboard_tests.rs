//! Rust-only tests of the runner's keyboard calls (`runner/keyboard.rs`) and
//! `SimulatedInput`'s keyboard members (`input.rs`): what a chord leaves
//! held, how pressed modifiers are carried and released, the close chord,
//! a type string naming no key, and `select_all`/`select_none`. The C#
//! tests that type are in `automation_runner_tests.rs` and
//! `text_edit_focus_tests.rs`.
//!
//! Keys are watched through the `App`'s global key handler, which sees
//! every key down nothing focused consumed (nothing is focused here except
//! in the select tests).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widgets::TextField;
use agg_gui::{Key, Modifiers, Rect, Widget};
use agg_gui_automation::tree_query::find_by_name;
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationError, AutomationWindow, Keys, ModifierKeys,
    RunOptions,
};

type KeyLog = Rc<RefCell<Vec<(Key, Modifiers)>>>;

/// A 300 × 200 window whose unconsumed key downs are logged.
fn logging_window() -> (AutomationWindow, KeyLog) {
    let log: KeyLog = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&log);
    let window = AutomationWindow::new(300.0, 200.0).on_load(move |app| {
        app.set_global_key_handler(move |key, mods| {
            sink.borrow_mut().push((key, mods));
            true
        });
    });
    (window, log)
}

fn command() -> Modifiers {
    Keys::CONTROL.to_agg_modifiers()
}

fn shift() -> Modifiers {
    Keys::SHIFT.to_agg_modifiers()
}

fn shift_command() -> Modifiers {
    (Keys::SHIFT | Keys::CONTROL).to_agg_modifiers()
}

#[test]
fn rust_only_a_chord_stroke_leaves_no_modifier_held() {
    show_window_and_execute_tests(RunOptions::default(), logging_window, |runner, log| {
        runner.type_text("^a");

        assert_eq!(*log.borrow(), vec![(Key::Char('a'), command())]);
        assert_eq!(
            runner.driver().forwarder().modifiers(),
            Modifiers::default()
        );
        assert_eq!(agg_gui::event::current_modifiers(), Modifiers::default());
        runner.mark_test_complete();
    })
    .expect("a chord's modifiers end with its stroke");
}

#[test]
fn rust_only_pressed_modifiers_are_held_carried_and_released_one_by_one() {
    show_window_and_execute_tests(RunOptions::default(), logging_window, |runner, log| {
        runner.press_modifier_keys(ModifierKeys::SHIFT | ModifierKeys::CONTROL);
        assert_eq!(
            *log.borrow(),
            vec![
                (Keys::SHIFT_KEY.to_agg_key(), shift_command()),
                (Keys::CONTROL_KEY.to_agg_key(), shift_command()),
            ],
            "each pressed modifier key goes down with the new held set"
        );
        assert_eq!(runner.driver().forwarder().modifiers(), shift_command());
        assert_eq!(agg_gui::event::current_modifiers(), shift_command());

        log.borrow_mut().clear();
        runner.type_text("x");
        assert_eq!(*log.borrow(), vec![(Key::Char('x'), shift_command())]);
        assert_eq!(
            runner.driver().forwarder().modifiers(),
            shift_command(),
            "a stroke typed under held modifiers leaves them held"
        );

        // C# dropped every held modifier on any release; only Shift goes.
        runner.release_modifier_keys(ModifierKeys::SHIFT);
        assert_eq!(runner.driver().forwarder().modifiers(), command());
        assert_eq!(agg_gui::event::current_modifiers(), command());

        runner.release_modifier_keys(ModifierKeys::CONTROL);
        assert_eq!(
            runner.driver().forwarder().modifiers(),
            Modifiers::default()
        );
        assert_eq!(agg_gui::event::current_modifiers(), Modifiers::default());
        runner.mark_test_complete();
    })
    .expect("held modifiers are carried by strokes and released individually");
}

#[test]
fn rust_only_modifier_none_sends_nothing() {
    show_window_and_execute_tests(RunOptions::default(), logging_window, |runner, log| {
        let frames = runner.driver().frames();
        runner.press_modifier_keys(ModifierKeys::NONE);
        runner.release_modifier_keys(ModifierKeys::NONE);

        assert!(log.borrow().is_empty());
        assert_eq!(
            runner.driver().frames(),
            frames,
            "no delay either, as in C#"
        );
        runner.mark_test_complete();
    })
    .expect("ModifierKeys::NONE is a no-op");
}

#[test]
fn rust_only_a_shift_press_reports_shift_alone() {
    show_window_and_execute_tests(RunOptions::default(), logging_window, |runner, log| {
        runner.press_modifier_keys(ModifierKeys::SHIFT);
        assert_eq!(*log.borrow(), vec![(Keys::SHIFT_KEY.to_agg_key(), shift())]);
        runner.release_modifier_keys(ModifierKeys::SHIFT);
        runner.mark_test_complete();
    })
    .expect("a Shift press is the Shift key with Shift held");
}

#[test]
fn rust_only_close_chord_asks_the_window_to_close_and_types_nothing() {
    show_window_and_execute_tests(RunOptions::default(), logging_window, |runner, log| {
        assert!(!runner.driver().close_requested());
        runner.type_text("%{F4}");

        assert!(runner.driver().close_requested());
        assert!(log.borrow().is_empty(), "Alt+F4 is not delivered as keys");
        runner.mark_test_complete();
    })
    .expect("%{F4} asks the window to close");
}

#[test]
fn rust_only_a_type_string_naming_no_key_fails_the_test() {
    let result =
        show_window_and_execute_tests(RunOptions::default(), logging_window, |runner, _| {
            runner.type_text("{NoSuchKey}");
            runner.mark_test_complete();
        });

    match result {
        Err(AutomationError::BodyPanicked(message)) => {
            assert!(message.contains("does not name a key"), "{message}")
        }
        other => panic!("expected the body to fail, got {other:?}"),
    }
}

#[test]
fn rust_only_select_all_then_select_none_replaces_the_text_with_a_space() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let mut window = AutomationWindow::new(300.0, 200.0);
            let mut field = TextField::new(Arc::new(agg_gui::fonts::standard_ui_font()))
                .with_text("Some Text")
                .with_name("field");
            field.set_bounds(Rect::new(50.0, 90.0, 200.0, 24.0));
            window.add_child(Box::new(field));
            (window, ())
        },
        |runner, _| {
            runner.click_by_name("field");
            runner.select_all();
            let selected = field(runner).selection();
            assert_eq!(selected, "Some Text");

            runner.select_none();
            assert_eq!(field(runner).text(), " ");
            assert_eq!(field(runner).selection(), "");
            runner.mark_test_complete();
        },
    )
    .expect("select_all selects everything and select_none types a space over it");
}

fn field(runner: &agg_gui_automation::AutomationRunner) -> &TextField {
    let root = runner.app().root();
    find_by_name(root, "field")
        .first()
        .and_then(|handle| handle.downcast::<TextField>(root))
        .expect("the field is in the window")
}
