//! Port of agg-sharp `Tests/Agg.Tests/Agg Automation Tests/AutomationRunnerTests.Winforms.cs`
//! RealMouseInputDoesNotReachAWindowUnderAutomation, on a real window: the
//! runner drives a `LiveDriver` (an `agg-gui-shell` window pumped through
//! `ShellSession`), and real input arrives the way the OS hands it to the
//! shell. C# sends Win32 messages to the window with `SendMessage`; here a
//! real mouse move and leave are winit `WindowEvent`s run through the shell's
//! own winit arms, and the real keys enter where the shell's `KeyboardInput`
//! arm hands them on (winit's `KeyEvent` cannot be built outside winit), with
//! the typed character as an input-method commit, as WM_CHAR's text.
//!
//! agg-gui sends keys to the focused widget, where C#'s `SystemWindow` hears
//! every key itself, so the test focuses a probe with a simulated click
//! before it starts and counts what reaches that probe, plus the key downs
//! the window's unconsumed-key handler hears.
//!
//! A `harness = false` binary: winit wants its event loop on the main thread
//! (macOS) and allows one per process. Where no window or GPU can come up
//! (no display, no adapter) it says why and passes, as the headless GPU tests
//! of agg-gui-wgpu do.

use std::cell::Cell;
use std::process::ExitCode;
use std::rc::Rc;

use agg_gui::event::Event;
use agg_gui::shell_input::ForwarderEvent;
use agg_gui::{Key, Modifiers, Rect, UnderMouseState};
use agg_gui_automation::tree_query::find_by_name;
use agg_gui_automation::{
    AutomationConfig, AutomationRunner, ClickOpts, FrameKind, LiveDriver, ProbeLog, ProbeWidget,
    UiDriver,
};
use agg_gui_shell::winit::dpi::PhysicalPosition;
use agg_gui_shell::winit::event::{DeviceId, Ime, WindowEvent};
use agg_gui_shell::ShellConfig;

/// The target's state, read as C# reads `target.UnderMouseState`.
fn target_state(runner: &AutomationRunner<LiveDriver>) -> UnderMouseState {
    let target = find_by_name(runner.app().root(), "Target")
        .into_iter()
        .next()
        .expect("the target is in the tree");
    runner.under_mouse_state(&target)
}

/// Key events a probe has logged.
fn key_events(log: &ProbeLog) -> usize {
    log.borrow()
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::KeyDown { .. } | Event::KeyUp { .. } | Event::TextInput { .. }
            )
        })
        .count()
}

fn real_mouse_input_does_not_reach_a_window_under_automation() -> Result<(), String> {
    // C#: a 300 x 200 window with a centred 100 x 100 "Target".
    let keyboard = ProbeWidget::new("Keyboard")
        .with_bounds(Rect::new(0.0, 0.0, 40.0, 40.0))
        .with_focusable(true);
    let keyboard_log = keyboard.log();
    let root = ProbeWidget::new("Window")
        .with_child(Box::new(
            ProbeWidget::new("Target").with_bounds(Rect::new(100.0, 50.0, 100.0, 100.0)),
        ))
        .with_child(Box::new(keyboard));
    let config = ShellConfig::new("RealMouseInputDoesNotReachAWindowUnderAutomation")
        .with_logical_size(300.0, 200.0);
    let driver = match LiveDriver::open(config, Box::new(root)) {
        Ok(driver) => driver,
        Err(error) => {
            println!("skipped: no live window can come up here ({error})");
            return Ok(());
        }
    };
    let mut runner = AutomationRunner::attach(driver, AutomationConfig::default());
    runner.wait_for_draw();

    let unconsumed_keys = Rc::new(Cell::new(0usize));
    let counted = Rc::clone(&unconsumed_keys);
    runner.app_mut().set_global_key_handler(move |_, _| {
        counted.set(counted.get() + 1);
        true
    });
    runner.click_by_name("Keyboard");

    runner.move_to_by_name("Target", &ClickOpts::default());
    runner.wait_for(
        |r| target_state(r) != UnderMouseState::NotUnderMouse,
        5.0,
        10,
    );

    // Each real event is handled by the next pump, so the state read after
    // it already reflects the event (C#'s SendMessage returns once handled).
    let real = |runner: &mut AutomationRunner<LiveDriver>, event: WindowEvent| {
        runner.driver_mut().session_mut().queue_window_event(event);
        runner.driver_mut().pump(FrameKind::Reactive);
    };
    real(
        &mut runner,
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(2.0, 2.0),
        },
    );
    let after_real_move = target_state(&runner);
    real(
        &mut runner,
        WindowEvent::CursorLeft {
            device_id: DeviceId::dummy(),
        },
    );
    let after_real_leave = target_state(&runner);

    // A user who clicks a test window to watch it can then type into it.
    let keys_before = key_events(&keyboard_log) + unconsumed_keys.get();
    let session = runner.driver_mut().session_mut();
    session.queue_platform_input(ForwarderEvent::KeyDown {
        key: Key::Char('a'),
        modifiers: Some(Modifiers::default()),
    });
    session.queue_window_event(WindowEvent::Ime(Ime::Commit("a".to_string())));
    session.queue_platform_input(ForwarderEvent::KeyUp {
        key: Key::Char('a'),
        modifiers: Some(Modifiers::default()),
    });
    runner.driver_mut().pump(FrameKind::Reactive);
    let real_keys = key_events(&keyboard_log) + unconsumed_keys.get() - keys_before;

    runner.mark_test_complete();
    drop(runner);

    if after_real_move == UnderMouseState::NotUnderMouse {
        return Err("a real mouse move must not move the pointer a test is driving".into());
    }
    if after_real_leave == UnderMouseState::NotUnderMouse {
        return Err(
            "a real mouse leave must not take the pointer off the widget a test moved to".into(),
        );
    }
    if real_keys != 0 {
        return Err(format!(
            "real keystrokes must not reach a window a test is typing into ({real_keys} did)"
        ));
    }
    Ok(())
}

fn main() -> ExitCode {
    let name = "real_mouse_input_does_not_reach_a_window_under_automation";
    match real_mouse_input_does_not_reach_a_window_under_automation() {
        Ok(()) => {
            println!("test {name} ... ok");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("test {name} ... FAILED: {message}");
            ExitCode::FAILURE
        }
    }
}
