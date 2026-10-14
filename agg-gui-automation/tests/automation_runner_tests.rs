//! Port of agg-sharp `Tests/Agg.Tests/Agg Automation Tests/AutomationRunnerTests.cs`
//! and the headless part of its WinForms half (`AutomationRunnerTests.Winforms.cs`);
//! the WinForms tests that need a live window are listed in
//! `docs/design/gui-automation.md`. C#'s `SystemWindow` is an
//! [`AutomationWindow`], built on the run's own UI thread; C#'s captured
//! locals are shared atomics when the calling thread sets them, and state
//! `build` returns beside the window (a click counter) when the widgets do,
//! as both the widgets and the body run on that thread.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use agg_gui::event::{self, Event, EventResult};
use agg_gui::widgets::{Button, TextArea};
use agg_gui::{Color, Font, Framebuffer, GfxCtx, Rect, Size, Widget};
use agg_gui_automation::runner::{
    DEFAULT_CHECK_INTERVAL_MILLISECONDS, DEFAULT_WIDGET_WAIT_SECONDS,
};
use agg_gui_automation::tree_query::find_by_name;
use agg_gui_automation::WaitOpts;
use agg_gui_automation::{
    show_window_and_execute_tests, static_delay, AutomationError, AutomationRunner,
    AutomationWindow, FrameKind, HeadlessDriver, ProbeWidget, RunOptions, ScreenRectangle,
    SearchRegion, UiDriver,
};

#[test]
fn get_widget_by_name_test_no_region_single_window() {
    // single system window
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let left_click_count = Rc::new(Cell::new(0));

            let mut button_container = AutomationWindow::new(300.0, 200.0);

            let font = Arc::new(agg_gui::fonts::standard_ui_font());
            let clicks = Rc::clone(&left_click_count);
            let left_button = left_button(font).on_click(move || clicks.set(clicks.get() + 1));
            button_container.add_child(Box::new(left_button));
            (button_container, left_click_count)
        },
        |test_runner, left_click_count| {
            test_runner.click_by_name("left");
            test_runner.delay(0.5);

            assert!(left_click_count.get() == 1);
            test_runner.mark_test_complete();
        },
    )
    .expect("clicking a button by name clicks it once");
}

#[test]
fn static_delay_expires_on_total_elapsed_time_not_the_seconds_component() {
    let timer = Instant::now();

    let satisfied = static_delay(|| false, 0.2, 10);

    assert!(!satisfied);
    assert!(
        timer.elapsed().as_secs_f64() < 0.9,
        "a .2 second wait must expire on total elapsed time, not on whole seconds ticking over"
    );
}

/// A zero-second wait is a single look: it answers whether the widget is
/// there right now.
///
/// The waits used to decide their answer by the clock (any elapsed time past
/// secondsToWait meant "not found"), so with 0 seconds they reported a
/// missing widget even when it was on screen.
#[test]
fn zero_second_waits_report_what_is_there_now() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let mut system_window = AutomationWindow::new(300.0, 200.0);

            let font = Arc::new(agg_gui::fonts::standard_ui_font());
            system_window.add_child(Box::new(placed_button("present", 10.0, 40.0, font)));
            (system_window, ())
        },
        |test_runner, _| {
            test_runner.wait_for_name("present", DEFAULT_WIDGET_WAIT_SECONDS);

            assert!(
                test_runner.wait_for_name("present", 0.0),
                "the widget is on screen, so a single look finds it"
            );
            assert!(
                test_runner.name_exists("present", 0.0, true),
                "NameExists is WaitForName by another name"
            );
            assert!(
                !test_runner.wait_for_name("absent", 0.0),
                "no widget has that name"
            );
            assert!(
                test_runner.wait_for_widget_disappear("absent", 0.0),
                "a widget that is not there has already disappeared"
            );
            assert!(
                !test_runner.wait_for_widget_disappear("present", 0.0),
                "the widget is still on screen"
            );

            assert!(
                static_delay(|| true, 0.0, 10),
                "a condition already met is met, however short the wait"
            );

            test_runner.wait_for_widget_enabled("present", 0.0);

            test_runner.mark_test_complete();
        },
    )
    .expect("zero-second waits report what is there now");
}

/// The test's budget measures the test, not the window coming up before it.
///
/// Bringing a window up creates its GPU device, and on a loaded machine
/// bring-up was measured at up to 14s, almost all of it wgpu device creation,
/// while the PopupAnchorTests body it preceded took about 3s - so a 25s test
/// that needed 3s timed out in the full suite. The slow Load here stands in
/// for that device creation: the body itself is instant, so the only way this
/// run can time out is if the load is charged to the test.
#[test]
fn window_load_time_is_not_charged_to_the_test_budget() {
    let body_ran = Arc::new(AtomicBool::new(false));
    let body_flag = Arc::clone(&body_ran);

    show_window_and_execute_tests(
        RunOptions::with_budget(1.0),
        || {
            let system_window = AutomationWindow::new(300.0, 200.0)
                .on_load(|_| thread::sleep(Duration::from_millis(1500)));
            (system_window, ())
        },
        move |test_runner, _| {
            body_flag.store(true, Ordering::SeqCst);
            test_runner.mark_test_complete();
        },
    )
    .expect("the run must not be charged for its window's load");

    assert!(body_ran.load(Ordering::SeqCst));
}

#[test]
fn automation_runner_timeout_test() {
    // Ensure AutomationRunner returns timeout errors
    let result = show_window_and_execute_tests(
        RunOptions {
            // Timeout after 1 second
            secs_to_test_failure: 1.0,
            // The timeout is what this test asserts, so the runner must not
            // treat the run as a hang worth reporting.
            timeout_is_the_expected_outcome: true,
            ..RunOptions::default()
        },
        || {
            let mut system_window = AutomationWindow::new(300.0, 200.0);

            let font = Arc::new(agg_gui::fonts::standard_ui_font());
            system_window.add_child(Box::new(left_button(font)));
            (system_window, ())
        },
        |test_runner, _| {
            // Test method that runs for 10+ seconds
            thread::sleep(Duration::from_secs(10));
            test_runner.mark_test_complete();
        },
    );

    // Should have returned a timeout
    assert_eq!(result.err(), Some(AutomationError::Timeout));
}

#[test]
fn an_expected_timeout_leaves_no_thread_dump() {
    let dump_directory = std::env::current_dir()
        .expect("the test has a current directory")
        .join("TestResults");
    let before = thread_dumps_in(&dump_directory);

    let result = show_window_and_execute_tests(
        RunOptions {
            secs_to_test_failure: 1.0,
            timeout_is_the_expected_outcome: true,
            ..RunOptions::default()
        },
        || (AutomationWindow::new(300.0, 200.0), ()),
        |test_runner, _| {
            thread::sleep(Duration::from_secs(3));
            test_runner.mark_test_complete();
        },
    );
    match result {
        // The point of the run; the assertion is about what it did not leave behind.
        Ok(()) | Err(AutomationError::Timeout) => {}
        Err(other) => panic!("the run failed for another reason: {other}"),
    }

    let after = thread_dumps_in(&dump_directory);
    let new_dumps: Vec<&PathBuf> = after.difference(&before).collect();
    assert!(
        new_dumps.is_empty(),
        "a timeout the test is asserting is a result, not a hang to photograph: {new_dumps:?}"
    );
}

/// C# `Directory.GetFiles(dumpDirectory, "threadstacks-*.txt")`, empty when
/// the directory does not exist.
fn thread_dumps_in(directory: &Path) -> HashSet<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return HashSet::new();
    };
    entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("threadstacks-") && name.ends_with(".txt"))
        })
        .collect()
}

/// From the WinForms half of the class (`AutomationRunnerTests.Winforms.cs`).
/// C#'s subject is WinformsSystemWindow's process-wide idle timer, which
/// tearing down one window (`ResetFirstWindowFlag`) stopped for every other
/// live window. agg-gui has no such timer, but its idle pump is shared too:
/// the process's main queue passes between UI threads as their runs end, and
/// windows on one UI thread share its queue and virtual clock. So "another
/// window's teardown" is both of what that means here: another test's run
/// ending (its window closed, its UI thread gone) and a second window on this
/// run's own thread dropped. Neither may leave this window with a dead pump.
#[test]
fn idle_pump_survives_another_windows_teardown() {
    let idle_action_ran = Arc::new(AtomicBool::new(false));
    let ran = Arc::clone(&idle_action_ran);

    show_window_and_execute_tests(
        RunOptions {
            secs_to_test_failure: 30.0,
            close_window_timeout: 5.0,
            ..RunOptions::default()
        },
        || (AutomationWindow::new(300.0, 200.0), ()),
        move |test_runner, _| {
            let this_windows_pump = agg_gui::ui_thread::current_queue();

            // Exactly what the runner does when another test finishes while this window is live.
            show_window_and_execute_tests(
                RunOptions {
                    close_window_timeout: 5.0,
                    ..RunOptions::default()
                },
                || (AutomationWindow::new(300.0, 200.0), ()),
                |other_runner, _| other_runner.mark_test_complete(),
            )
            .expect("the other test's run passes");
            let mut other_window =
                HeadlessDriver::new(Box::new(ProbeWidget::new("Other Window")), 300, 200);
            other_window.pump(FrameKind::Forced);
            drop(other_window);
            // Checked as well as the action below, which a queue swapped
            // under this thread can still run when it adopts the main queue.
            assert!(
                agg_gui::ui_thread::current_queue().same_queue(&this_windows_pump),
                "another window's teardown must leave this window on its own pump"
            );

            let flag = Arc::clone(&ran);
            agg_gui::ui_thread::run_on_idle(move || flag.store(true, Ordering::SeqCst));
            test_runner.wait_for(
                |_| ran.load(Ordering::SeqCst),
                5.0,
                DEFAULT_CHECK_INTERVAL_MILLISECONDS,
            );

            test_runner.mark_test_complete();
        },
    )
    .expect("the run passes");

    assert!(
        idle_action_ran.load(Ordering::SeqCst),
        "a live window's RunOnIdle pump must survive another window's teardown"
    );
}

#[test]
fn get_widget_by_name_test_region_single_window() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let left_click_count = Rc::new(Cell::new(0));

            let mut button_container = AutomationWindow::new(300.0, 200.0);

            let font = Arc::new(agg_gui::fonts::standard_ui_font());
            let clicks = Rc::clone(&left_click_count);
            let left_button =
                left_button(Arc::clone(&font)).on_click(move || clicks.set(clicks.get() + 1));
            button_container.add_child(Box::new(left_button));

            let right_button = placed_button("right", 110.0, 40.0, font);
            button_container.add_child(Box::new(right_button));
            (button_container, left_click_count)
        },
        |test_runner, left_click_count| {
            test_runner.click_by_name("left");
            test_runner.delay(0.5);
            assert_eq!(left_click_count.get(), 1);

            assert!(test_runner.name_exists("left", DEFAULT_WIDGET_WAIT_SECONDS, true));

            let right_region = test_runner
                .get_region_by_name("right", &WaitOpts::default())
                .expect("the right button has a region");
            let widget = test_runner.get_widget_by_name(
                "left",
                &WaitOpts {
                    secs_to_wait: 5.0,
                    ..WaitOpts::in_region(&right_region)
                },
            );

            assert!(widget.is_none());
            test_runner.mark_test_complete();
        },
    )
    .expect("a name lookup limited to another widget's region finds nothing");
}

/// C# `new Button("left", 10, 40) { Name = "left" }`: a button at (10, 40)
/// at its own size.
fn left_button(font: Arc<Font>) -> Button {
    placed_button("left", 10.0, 40.0, font)
}

/// C# `new Button(text, x, y) { Name = text }`: a button named and labelled
/// `text` at (`x`, `y`), at its own size.
fn placed_button(text: &str, x: f64, y: f64, font: Arc<Font>) -> Button {
    let mut button = Button::new(text, font).with_name(text);
    let size = button.layout(Size::new(f64::MAX, f64::MAX));
    button.set_bounds(Rect::new(x, y, size.width, size.height));
    button
}

#[test]
fn double_click_by_name_sends_two_full_click_pairs_with_production_click_counts() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let events = Rc::new(RefCell::new(Vec::<String>::new()));

            let mut system_window = AutomationWindow::new(300.0, 200.0);

            // C#'s MouseDown/MouseUp subscriptions; the target takes the
            // press, as every C# GuiWidget does.
            let log = Rc::clone(&events);
            let target = ProbeWidget::new("target")
                .with_bounds(Rect::new(0.0, 0.0, 100.0, 100.0))
                .on_event_with(move |event| match event {
                    Event::MouseDown { .. } => {
                        log.borrow_mut()
                            .push(format!("down:{}", event::current_click_count()));
                        EventResult::Consumed
                    }
                    Event::MouseUp { .. } => {
                        let kind = if event::is_double_click() {
                            "double"
                        } else {
                            "single"
                        };
                        log.borrow_mut()
                            .push(format!("up:{}:{kind}", event::current_click_count()));
                        EventResult::Consumed
                    }
                    _ => EventResult::Ignored,
                });
            system_window.add_child(Box::new(target));
            (system_window, events)
        },
        |test_runner, events| {
            test_runner.click_by_name("target");

            // Joined so the assertion is order-exact - the SEQUENCE is the contract.
            assert_eq!(
                events.borrow().join(","),
                "down:1,up:1:single",
                "a single click is one press/release pair and must not read as a double click"
            );

            events.borrow_mut().clear();
            test_runner.double_click_by_name("target");

            assert_eq!(
                events.borrow().join(","),
                "down:1,up:1:single,down:2,up:1:double",
                "a double click is two full pairs; only the second down carries Clicks == 2, \
                 and the final up - which the platform reports with Clicks == 1 - must still \
                 answer IsDoubleClick"
            );

            test_runner.mark_test_complete();
        },
    )
    .expect("a double click by name is two full click pairs");
}

#[test]
fn type_delivers_punctuation_into_a_multi_line_field() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let mut system_window = AutomationWindow::new(300.0, 200.0);
            // C# `new TextEditWidget("", 10, 10, pixelWidth: 200, pixelHeight: 100,
            // multiLine: true) { Name = "field" }`.
            let mut field =
                TextArea::new(Arc::new(agg_gui::fonts::standard_ui_font())).with_name("field");
            field.set_bounds(Rect::new(10.0, 10.0, 200.0, 100.0));
            system_window.add_child(Box::new(field));
            (system_window, ())
        },
        |test_runner, _| {
            test_runner.click_by_name("field");
            test_runner.type_text("q!\"#$%&'()*+,-./ok");
            assert_eq!(text_area_text(test_runner, "field"), "q!\"#$%&'()*+,-./ok");

            // Select-all is "^a" - the same spelling Windows' SendKeys reads.
            test_runner.type_text("^a");
            test_runner.type_text("z");
            assert_eq!(text_area_text(test_runner, "field"), "z");

            test_runner.mark_test_complete();
        },
    )
    .expect("typed punctuation lands in a multi-line field and ^a selects it all");
}

/// C# `field.Text` for the `TextArea` named `name`.
fn text_area_text(test_runner: &AutomationRunner, name: &str) -> String {
    let root = test_runner.app().root();
    find_by_name(root, name)
        .first()
        .and_then(|handle| handle.downcast::<TextArea>(root))
        .map(TextArea::text)
        .unwrap_or_else(|| panic!("no TextArea named {name}"))
}

/// An image of `width` × `height` cleared to `color` (C#
/// `new ImageBuffer(w, h)` then `NewGraphics2D().Clear(color)`).
fn cleared_image(width: u32, height: u32, color: Color) -> Framebuffer {
    let mut image = Framebuffer::new(width, height);
    GfxCtx::new(&mut image).clear(color);
    image
}

/// Image waits search the region they are given and come back, zero seconds
/// or not.
///
/// WaitForImage looked through ImageExists with its default two seconds,
/// which waited through WaitForImage again - a stack overflow on any image
/// wait - and dropped the caller's region.
#[test]
fn image_waits_search_the_given_region() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || (AutomationWindow::new(100.0, 100.0), ()),
        |test_runner, _| {
            let mut haystack = cleared_image(40, 30, Color::white());
            {
                // C# `FillRectangle(10, 10, 16, 16, Black)`: left, bottom,
                // right, top.
                let mut graphics = GfxCtx::new(&mut haystack);
                graphics.set_fill_color(Color::black());
                graphics.begin_path();
                graphics.rect(10.0, 10.0, 6.0, 6.0);
                graphics.fill();
            }

            let black_needle = cleared_image(6, 6, Color::black());
            let red_needle = cleared_image(6, 6, Color::rgb(1.0, 0.0, 0.0));

            let region = SearchRegion::with_image(haystack, ScreenRectangle::new(0, 0, 40, 30));

            assert!(
                test_runner.wait_for_image(&black_needle, 0.0, Some(&region)),
                "the black square is in the given region"
            );
            assert!(
                test_runner.image_exists(&black_needle, 1.0, Some(&region)),
                "ImageExists with a wait looks the same way"
            );
            assert!(
                !test_runner.image_exists(&red_needle, 0.0, Some(&region)),
                "nothing red is in the region"
            );

            test_runner.mark_test_complete();
        },
    )
    .expect("image waits search the region they are given");
}

/// A window whose close handler cancels the close (the shape of a "do you
/// want to save?" prompt) used to hang the whole run: the close was vetoed,
/// the message pump never exited, and the test reported Passed only after a
/// human closed the app by hand. The close phase watchdog must force the
/// window closed and surface the hang as a failure.
#[test]
fn vetoed_close_is_forced_and_reported_as_failure() {
    let veto_count = Arc::new(AtomicUsize::new(0));
    let has_been_closed = Arc::new(AtomicBool::new(false));

    let vetoes = Arc::clone(&veto_count);
    let closed = Arc::clone(&has_been_closed);
    let result = show_window_and_execute_tests(
        RunOptions {
            secs_to_test_failure: 30.0,
            // C# sets the static CloseWindowTimeoutSeconds to 2 for this test.
            close_window_timeout: 2.0,
            ..RunOptions::default()
        },
        move || {
            let system_window = AutomationWindow::new(300.0, 200.0)
                .on_close_requested(move |_| {
                    // Always cancel - only a force close should be able to
                    // get past this.
                    vetoes.fetch_add(1, Ordering::SeqCst);
                    false
                })
                .on_closed(move |_| closed.store(true, Ordering::SeqCst));
            (system_window, ())
        },
        |test_runner, _| test_runner.mark_test_complete(),
    );

    assert_eq!(
        result,
        Err(AutomationError::CloseTimeout(Duration::from_secs(2))),
        "the vetoed close should have been reported as a close timeout"
    );
    assert!(
        veto_count.load(Ordering::SeqCst) > 0,
        "the test window must actually have vetoed a close for this to be a valid repro"
    );
    assert!(
        has_been_closed.load(Ordering::SeqCst),
        "the watchdog must force the window closed so the run does not hang"
    );
}
