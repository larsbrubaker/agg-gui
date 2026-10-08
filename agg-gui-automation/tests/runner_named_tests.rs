//! Rust-only tests of the runner's name lookups and the waits that poll them
//! (agg-gui-automation `src/runner/named.rs`): `get_widgets_by_name`,
//! `get_widget_by_name`, `get_region_by_name`, `name_exists`,
//! `named_widget_exists`, `child_exists`, `wait_for_name`,
//! `wait_for_widget_disappear`, `wait_for_widget_enabled` and
//! `widget_not_found_message`. The ported C# test that exercises them is
//! `zero_second_waits_report_what_is_there_now` in
//! `automation_runner_tests.rs`.

mod support;

use std::time::Duration;

use agg_gui::{clock, ui_thread, Rect, Widget};
use agg_gui_automation::search_region::{ScreenRectangle, SearchRegion};
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationError, AutomationRunner, AutomationWindow,
    ProbeWidget, RunOptions, WaitOpts,
};
use support::{set, switch, Switch, Switchable};

/// Run `body` against a 300 × 200 window holding `children`, with `state`
/// (the switches the body flips) handed through.
fn run_with<S: 'static, R: Send + 'static>(
    build: impl FnOnce() -> (Vec<Box<dyn Widget>>, S) + Send + 'static,
    body: impl FnOnce(&mut AutomationRunner, &S) -> R + Send + 'static,
) -> Result<R, AutomationError> {
    show_window_and_execute_tests(
        RunOptions::default(),
        move || {
            let (children, state) = build();
            let mut window = AutomationWindow::new(300.0, 200.0);
            for child in children {
                window.add_child(child);
            }
            (window, state)
        },
        move |runner, state| {
            let result = body(runner, state);
            runner.mark_test_complete();
            result
        },
    )
}

fn probe(name: &str, x: f64, y: f64, w: f64, h: f64) -> Box<dyn Widget> {
    Box::new(ProbeWidget::new(name).with_bounds(Rect::new(x, y, w, h)))
}

/// Flip `switch` to `on` after `secs` of UI time.
fn flip_after(switch: &Switch, secs: f64, on: bool) {
    let switch = Switch::clone(switch);
    ui_thread::run_on_idle_after(Duration::from_secs_f64(secs), move || set(&switch, on));
}

#[test]
fn rust_only_get_widgets_by_name_finds_every_visible_widget_of_that_name() {
    let found = run_with(
        || {
            let mut hidden = ProbeWidget::new("twin").with_bounds(Rect::new(0.0, 0.0, 10.0, 10.0));
            hidden.set_visible(false);
            let children: Vec<Box<dyn Widget>> = vec![
                probe("twin", 10.0, 10.0, 20.0, 20.0),
                probe("other", 40.0, 10.0, 20.0, 20.0),
                Box::new(hidden),
                probe("twin", 100.0, 100.0, 40.0, 30.0),
            ];
            (children, ())
        },
        |runner, _| {
            let visible = runner.get_widgets_by_name("twin", &WaitOpts::default());
            let all = runner.get_widgets_by_name(
                "twin",
                &WaitOpts {
                    only_visible: false,
                    ..WaitOpts::default()
                },
            );
            let paths: Vec<Vec<usize>> = visible.iter().map(|h| h.handle.path().to_vec()).collect();
            let hints: Vec<(f64, f64)> = visible
                .iter()
                .map(|h| (h.offset_hint.x, h.offset_hint.y))
                .collect();
            (paths, hints, all.len())
        },
    )
    .expect("run");

    assert_eq!(found.0, vec![vec![0], vec![3]]);
    assert_eq!(
        found.1,
        vec![(10.0, 10.0), (20.0, 15.0)],
        "the centre of each"
    );
    assert_eq!(found.2, 3, "the hidden one counts when visibility does not");
}

#[test]
fn rust_only_a_search_region_limits_the_lookup_to_widgets_it_overlaps() {
    let paths = run_with(
        || {
            let children = vec![
                probe("twin", 10.0, 10.0, 20.0, 20.0),
                probe("twin", 200.0, 150.0, 40.0, 30.0),
            ];
            (children, ())
        },
        |runner, _| {
            // The top-left quarter of the window, in Y-down pixels: it holds
            // the second widget (Y-up 150..180 is Y-down 20..50).
            let region = SearchRegion::new(ScreenRectangle::new(150, 0, 300, 100));
            runner
                .get_widgets_by_name("twin", &WaitOpts::in_region(&region))
                .iter()
                .map(|h| h.handle.path().to_vec())
                .collect::<Vec<_>>()
        },
    )
    .expect("run");

    assert_eq!(paths, vec![vec![1]]);
}

#[test]
fn rust_only_get_widget_by_name_waits_for_the_name_to_appear() {
    let (found, waited) = run_with(
        || {
            let visible = switch(false);
            let late =
                Switchable::new("late", Rect::new(10.0, 10.0, 20.0, 20.0)).visible_when(&visible);
            (vec![Box::new(late) as Box<dyn Widget>], visible)
        },
        |runner, visible| {
            flip_after(visible, 0.5, true);
            let started = clock::now();
            let found = runner.get_widget_by_name("late", &WaitOpts::default());
            (found.is_some(), clock::since(started))
        },
    )
    .expect("run");

    assert!(found);
    assert!(waited >= Duration::from_millis(500) && waited < Duration::from_millis(600));
}

#[test]
fn rust_only_get_widget_by_name_with_no_wait_does_not_pump() {
    let (found, frames) = run_with(
        || (Vec::new(), ()),
        |runner, _| {
            let frames = runner.driver().frames();
            let found = runner.get_widget_by_name("missing", &WaitOpts::secs(0.0));
            (found.is_some(), runner.driver().frames() - frames)
        },
    )
    .expect("run");

    assert!(!found);
    assert_eq!(frames, 0);
}

#[test]
fn rust_only_get_widget_by_name_prefers_the_largest_widget_of_that_name() {
    let path = run_with(
        || {
            let children = vec![
                probe("twin", 10.0, 10.0, 20.0, 20.0),
                probe("twin", 100.0, 100.0, 60.0, 40.0),
                probe("twin", 200.0, 10.0, 30.0, 30.0),
            ];
            (children, ())
        },
        |runner, _| {
            runner
                .get_widget_by_name("twin", &WaitOpts::default())
                .map(|h| h.path().to_vec())
        },
    )
    .expect("run");

    assert_eq!(path, Some(vec![1]));
}

#[test]
fn rust_only_get_widget_by_name_prefers_a_widget_a_press_can_reach() {
    let path = run_with(
        || {
            // The bigger one sits in a container that lets presses through.
            let fading = Switchable::new("fading window", Rect::new(0.0, 0.0, 200.0, 200.0))
                .passing_presses_through()
                .with_child(probe("twin", 10.0, 10.0, 100.0, 100.0));
            let children: Vec<Box<dyn Widget>> =
                vec![Box::new(fading), probe("twin", 220.0, 10.0, 20.0, 20.0)];
            (children, ())
        },
        |runner, _| {
            runner
                .get_widget_by_name("twin", &WaitOpts::default())
                .map(|h| h.path().to_vec())
        },
    )
    .expect("run");

    assert_eq!(path, Some(vec![1]));
}

#[test]
fn rust_only_get_region_by_name_is_the_widgets_rectangle_in_y_down_pixels() {
    let rect = run_with(
        || (vec![probe("box", 10.0, 40.0, 50.0, 20.0)], ()),
        |runner, _| {
            runner
                .get_region_by_name("box", &WaitOpts::default())
                .map(|region| region.screen_rect)
        },
    )
    .expect("run");

    // Y-up 40..60 in a 200-high window is Y-down 140..160.
    assert_eq!(rect, Some(ScreenRectangle::new(10, 140, 60, 160)));
}

#[test]
fn rust_only_named_widget_exists_honours_visibility_region_and_predicate() {
    let answers = run_with(
        || {
            let mut hidden =
                ProbeWidget::new("hidden").with_bounds(Rect::new(0.0, 0.0, 10.0, 10.0));
            hidden.set_visible(false);
            let children: Vec<Box<dyn Widget>> =
                vec![Box::new(hidden), probe("shown", 10.0, 10.0, 20.0, 20.0)];
            (children, ())
        },
        |runner, _| {
            let far_away = SearchRegion::new(ScreenRectangle::new(200, 0, 300, 50));
            let wide = |w: &dyn Widget| w.bounds().width > 15.0;
            let narrow = |w: &dyn Widget| w.bounds().width < 15.0;
            [
                runner.named_widget_exists("hidden", None, true, None),
                runner.named_widget_exists("hidden", None, false, None),
                runner.named_widget_exists("shown", None, true, None),
                runner.named_widget_exists("shown", Some(&far_away), true, None),
                runner.named_widget_exists("shown", None, true, Some(&wide)),
                runner.named_widget_exists("shown", None, true, Some(&narrow)),
            ]
        },
    )
    .expect("run");

    assert_eq!(answers, [false, true, true, false, true, false]);
}

#[test]
fn rust_only_child_exists_looks_at_the_windows_visible_children_by_type() {
    let answers = run_with(
        || {
            let hidden = Switchable::new("hidden", Rect::new(0.0, 0.0, 10.0, 10.0))
                .visible_when(&switch(false));
            let children: Vec<Box<dyn Widget>> =
                vec![probe("probe", 10.0, 10.0, 20.0, 20.0), Box::new(hidden)];
            (children, ())
        },
        |runner, _| {
            let far_away = SearchRegion::new(ScreenRectangle::new(200, 0, 300, 50));
            [
                runner.child_exists::<ProbeWidget>(None),
                runner.child_exists::<ProbeWidget>(Some(&far_away)),
                runner.child_exists::<Switchable>(None),
            ]
        },
    )
    .expect("run");

    assert_eq!(answers, [true, false, false]);
}

#[test]
fn rust_only_wait_for_name_polls_a_frame_at_a_time_until_its_time_is_up() {
    let (found, frames, waited) = run_with(
        || (Vec::new(), ()),
        |runner, _| {
            let frames = runner.driver().frames();
            let started = clock::now();
            let found = runner.wait_for_name("absent", 0.5);
            (
                found,
                runner.driver().frames() - frames,
                clock::since(started),
            )
        },
    )
    .expect("run");

    assert!(!found);
    assert_eq!(frames, 50, "one 10 ms frame per look");
    assert_eq!(waited, Duration::from_millis(500));
}

#[test]
fn rust_only_wait_for_name_with_a_predicate_waits_for_it_to_hold() {
    let found = run_with(
        || {
            let enabled = switch(false);
            let button =
                Switchable::new("button", Rect::new(10.0, 10.0, 20.0, 20.0)).enabled_when(&enabled);
            (vec![Box::new(button) as Box<dyn Widget>], enabled)
        },
        |runner, enabled| {
            flip_after(enabled, 0.2, true);
            let is_enabled = |w: &dyn Widget| w.is_enabled();
            let early = runner.wait_for_name_with("button", 0.1, true, Some(&is_enabled));
            let late = runner.wait_for_name_with("button", 2.0, true, Some(&is_enabled));
            (early, late)
        },
    )
    .expect("run");

    assert_eq!(found, (false, true));
}

#[test]
fn rust_only_wait_for_widget_disappear_waits_for_the_widget_to_go() {
    let gone = run_with(
        || {
            let visible = switch(true);
            let leaving = Switchable::new("leaving", Rect::new(10.0, 10.0, 20.0, 20.0))
                .visible_when(&visible);
            (vec![Box::new(leaving) as Box<dyn Widget>], visible)
        },
        |runner, visible| {
            flip_after(visible, 0.3, false);
            let early = runner.wait_for_widget_disappear("leaving", 0.1);
            let late = runner.wait_for_widget_disappear("leaving", 2.0);
            (early, late)
        },
    )
    .expect("run");

    assert_eq!(gone, (false, true));
}

#[test]
fn rust_only_wait_for_widget_enabled_waits_for_the_widget_and_its_ancestors() {
    let result = run_with(
        || {
            let enabled = switch(false);
            // The child is enabled itself; its parent is what is disabled.
            let parent = Switchable::new("parent", Rect::new(0.0, 0.0, 200.0, 200.0))
                .enabled_when(&enabled)
                .with_child(Box::new(Switchable::new(
                    "child",
                    Rect::new(10.0, 10.0, 20.0, 20.0),
                )));
            (vec![Box::new(parent) as Box<dyn Widget>], enabled)
        },
        |runner, enabled| {
            flip_after(enabled, 0.3, true);
            let started = clock::now();
            runner.wait_for_widget_enabled("child", 2.0);
            clock::since(started)
        },
    )
    .expect("the child becomes enabled with its parent");

    assert!(result >= Duration::from_millis(300));
}

#[test]
fn rust_only_wait_for_widget_enabled_fails_with_csharps_message_when_it_stays_disabled() {
    let result = run_with(
        || {
            let disabled = Switchable::new("stuck", Rect::new(10.0, 10.0, 20.0, 20.0))
                .enabled_when(&switch(false));
            (vec![Box::new(disabled) as Box<dyn Widget>], ())
        },
        |runner, _| {
            runner.wait_for_widget_enabled("stuck", 0.5);
        },
    );

    assert_eq!(
        result,
        Err(AutomationError::BodyPanicked(
            "WaitForWidgetEnabled Failed: [stuck] not visible and enabled after [0.5] seconds"
                .to_string()
        ))
    );
}

#[test]
fn rust_only_wait_for_widget_enabled_fails_with_the_not_found_message_for_a_missing_widget() {
    let result = run_with(
        || (Vec::new(), ()),
        |runner, _| {
            runner.wait_for_widget_enabled("missing", 0.0);
        },
    );

    assert_eq!(
        result,
        Err(AutomationError::BodyPanicked(
            AutomationRunner::widget_not_found_message("WaitForWidgetEnabled", "missing")
        ))
    );
}

#[test]
fn rust_only_widget_not_found_message_names_the_operation_and_the_widget() {
    assert_eq!(
        AutomationRunner::widget_not_found_message("ClickByName", "Save Button"),
        "ClickByName Failed: Named GuiWidget not found [Save Button]"
    );
}
