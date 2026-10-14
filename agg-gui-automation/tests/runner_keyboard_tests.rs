//! Rust-only tests of the runner's keyboard calls (`runner/keyboard.rs`) and
//! `SimulatedInput`'s keyboard members (`input.rs`): what a chord leaves
//! held, how pressed modifiers are carried and released, physical Control
//! and Meta apart from the command modifier, the close chord,
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

/// `ModifierKeys::PHYSICAL_CONTROL` holds agg-gui's ctrl and `META` its meta
/// on every platform, apart from `CONTROL` (the command modifier: meta on
/// macOS, ctrl elsewhere), so a test can tell Control from Cmd on a Mac.
#[test]
fn rust_only_physical_control_and_meta_are_held_apart_from_the_command_modifier() {
    use agg_gui::platform::{override_platform_for_thread, Platform};

    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    let meta = Modifiers {
        meta: true,
        ..Modifiers::default()
    };
    for platform in [Platform::MacOS, Platform::Windows] {
        show_window_and_execute_tests(RunOptions::default(), logging_window, move |runner, log| {
            let _platform = override_platform_for_thread(platform);
            runner.press_modifier_keys(ModifierKeys::PHYSICAL_CONTROL);
            runner.type_text("{Up}");
            runner.release_modifier_keys(ModifierKeys::PHYSICAL_CONTROL);
            assert_eq!(
                *log.borrow(),
                vec![(Key::Other("Control".into()), ctrl), (Key::ArrowUp, ctrl),],
                "{platform:?}: the physical Control key goes down and carries ctrl"
            );
            assert_eq!(
                runner.driver().forwarder().modifiers(),
                Modifiers::default()
            );

            log.borrow_mut().clear();
            runner.press_modifier_keys(ModifierKeys::META);
            runner.type_text("{Up}");
            assert_eq!(
                *log.borrow(),
                vec![(Key::Other("Super".into()), meta), (Key::ArrowUp, meta)],
                "{platform:?}: the Meta key goes down and carries meta"
            );

            // Held with the command modifier, each keeps its own flag.
            log.borrow_mut().clear();
            runner.press_modifier_keys(ModifierKeys::PHYSICAL_CONTROL);
            runner.type_text("^{Down}");
            let held = Modifiers {
                ctrl: true,
                meta: true,
                ..Modifiers::default()
            };
            assert_eq!(log.borrow().last(), Some(&(Key::ArrowDown, held)));
            runner.release_modifier_keys(ModifierKeys::META);
            assert_eq!(runner.driver().forwarder().modifiers(), ctrl);
            runner.release_modifier_keys(ModifierKeys::PHYSICAL_CONTROL);
            assert_eq!(
                runner.driver().forwarder().modifiers(),
                Modifiers::default()
            );
            assert_eq!(agg_gui::event::current_modifiers(), Modifiers::default());
            runner.mark_test_complete();
        })
        .expect("physical Control and Meta are held as themselves");
    }
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

/// `type_text_with` and `TypeOpts { delay_after: 0.0 }`: the stroke is
/// delivered and one frame drawn, and no further UI time passes, so a test
/// can see a state that lasts one frame (a camera move just started, a
/// debounce still pending). The default is C#'s `Type`: `delay(0.2)` after.
#[test]
fn rust_only_typing_without_a_delay_draws_once_and_lets_no_more_ui_time_pass() {
    use agg_gui_automation::TypeOpts;

    assert_eq!(
        TypeOpts::default().delay_after,
        0.2,
        "C#'s Type delays 0.2 s"
    );
    show_window_and_execute_tests(RunOptions::default(), logging_window, |runner, log| {
        let frames = runner.driver().frames();
        let started = agg_gui::clock::now();
        runner.type_text_with("x", &TypeOpts { delay_after: 0.0 });
        assert_eq!(*log.borrow(), vec![(Key::Char('x'), Modifiers::default())]);
        assert_eq!(runner.driver().frames(), frames + 1, "exactly one frame");
        let one_frame = agg_gui::clock::since(started);
        assert!(one_frame.as_secs_f64() < 0.2, "{one_frame:?}");

        let frames = runner.driver().frames();
        let started = agg_gui::clock::now();
        runner.type_text("y");
        assert!(runner.driver().frames() > frames + 1);
        assert!(agg_gui::clock::since(started).as_secs_f64() >= 0.2);
        runner.mark_test_complete();
    })
    .expect("a zero delay_after draws once and returns");
}
