//! Rust-only tests of the runner's file drags and drops
//! (`runner/file_drop.rs`): a drop by path or by contents lands on the named
//! widget at its center after a stepped pointer move, and a drag hover then
//! leave reaches the widget in order.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use agg_gui::event::{Event, EventResult};
use agg_gui::{DroppedFileData, Point, Rect};
use agg_gui_automation::runner::mouse_move_steps;
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationWindow, DragOpts, InputMethod, Keys, MouseAction,
    Point2D, ProbeLog, ProbeWidget, RunOptions, SimulatedInput, TypedKey,
};

/// A [`SimulatedInput`] that records every pointer position it is sent.
struct RecordingInput {
    inner: SimulatedInput,
    moves: Rc<RefCell<Vec<Point2D>>>,
}

impl InputMethod for RecordingInput {
    fn current_mouse_position(&self) -> Point2D {
        self.inner.current_mouse_position()
    }

    fn left_button_down(&self) -> bool {
        self.inner.left_button_down()
    }

    fn click_count(&self) -> u32 {
        self.inner.click_count()
    }

    fn set_cursor_position(
        &mut self,
        driver: &mut dyn agg_gui_automation::driver::UiDriver,
        x: i32,
        y: i32,
    ) {
        self.moves.borrow_mut().push(Point2D::new(x, y));
        self.inner.set_cursor_position(driver, x, y);
    }

    fn mouse_event(
        &mut self,
        driver: &mut dyn agg_gui_automation::driver::UiDriver,
        action: MouseAction,
        x: i32,
        y: i32,
        clicks: u32,
    ) {
        self.inner.mouse_event(driver, action, x, y, clicks);
    }

    fn press_modifier_keys(
        &mut self,
        driver: &mut dyn agg_gui_automation::driver::UiDriver,
        modifier_keys: Keys,
    ) {
        self.inner.press_modifier_keys(driver, modifier_keys);
    }

    fn release_modifier_keys(
        &mut self,
        driver: &mut dyn agg_gui_automation::driver::UiDriver,
        modifier_keys: Keys,
    ) {
        self.inner.release_modifier_keys(driver, modifier_keys);
    }

    fn type_strokes(
        &mut self,
        driver: &mut dyn agg_gui_automation::driver::UiDriver,
        strokes: &[TypedKey],
    ) {
        self.inner.type_strokes(driver, strokes);
    }
}

/// A 300 × 200 window holding a drop target named `bin` at (50, 40) sized
/// 80 × 20 (Y-up), which takes every file event, beside a decoy named
/// `decoy` that takes them too.
fn window_with_bin() -> (AutomationWindow, (ProbeLog, ProbeLog)) {
    let takes_files = |event: &Event| match event {
        Event::FileDropped { .. } | Event::FileDataDropped { .. } | Event::FileDragHover { .. } => {
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    };
    let mut window = AutomationWindow::new(300.0, 200.0);
    let bin = ProbeWidget::new("bin")
        .with_bounds(Rect::new(50.0, 40.0, 80.0, 20.0))
        .on_event_with(takes_files);
    let decoy = ProbeWidget::new("decoy")
        .with_bounds(Rect::new(200.0, 100.0, 50.0, 50.0))
        .on_event_with(takes_files);
    let logs = (bin.log(), decoy.log());
    window.add_child(Box::new(bin));
    window.add_child(Box::new(decoy));
    (window, logs)
}

/// The file events in `log`, without the pointer's own.
fn file_events(log: &ProbeLog) -> Vec<Event> {
    log.borrow()
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::FileDropped { .. }
                    | Event::FileDataDropped { .. }
                    | Event::FileDragHover { .. }
                    | Event::FileDragLeave
            )
        })
        .cloned()
        .collect()
}

#[test]
fn rust_only_drop_files_by_name_delivers_the_paths_at_the_target_after_a_stepped_move() {
    show_window_and_execute_tests(
        RunOptions::default(),
        window_with_bin,
        |runner, (bin, decoy)| {
            let moves = Rc::new(RefCell::new(Vec::new()));
            runner.set_input_method(Box::new(RecordingInput {
                inner: SimulatedInput::new(),
                moves: Rc::clone(&moves),
            }));
            let start = runner.current_mouse_position();
            let paths = vec![PathBuf::from("/parts/a.stl"), PathBuf::from("b.mcx")];
            runner.drop_files_by_name("bin", &DragOpts::default(), &paths);

            // The bin spans x 50..130, y 40..60 (Y-up) in a 200-high window:
            // its center (90, 50) is pointer (90, 150), reached in the
            // runner's eased steps.
            let end = Point2D::new(90, 150);
            assert_eq!(runner.current_mouse_position(), end);
            assert_eq!(
                *moves.borrow(),
                mouse_move_steps(start, end, runner.config.mouse_move_steps)
            );
            match file_events(bin).as_slice() {
                [Event::FileDropped { pos, paths: got }] => {
                    assert_eq!(*pos, Point::new(40.0, 10.0));
                    assert_eq!(*got, paths);
                }
                other => panic!("one drop on the bin, got {other:?}"),
            }
            assert!(file_events(decoy).is_empty());
            runner.mark_test_complete();
        },
    )
    .expect("the drop lands");
}

#[test]
fn rust_only_drop_file_data_by_name_delivers_the_bytes() {
    show_window_and_execute_tests(
        RunOptions::default(),
        window_with_bin,
        |runner, (bin, _decoy)| {
            let opts = DragOpts {
                offset: Point2D::new(10, 5),
                origin: agg_gui_automation::ClickOrigin::LowerLeft,
                ..DragOpts::default()
            };
            runner.drop_file_data_by_name(
                "bin",
                &opts,
                vec![DroppedFileData::new("note.txt", b"hello".to_vec())],
            );
            match file_events(bin).as_slice() {
                [Event::FileDataDropped { pos, files }] => {
                    assert_eq!(*pos, Point::new(10.0, 5.0));
                    assert_eq!(files.len(), 1);
                    assert_eq!(files[0].name, "note.txt");
                    assert_eq!(&*files[0].bytes, b"hello");
                }
                other => panic!("one data drop on the bin, got {other:?}"),
            }
            runner.mark_test_complete();
        },
    )
    .expect("the data drop lands");
}

#[test]
fn rust_only_file_drag_hover_then_leave_reach_the_widget_in_order() {
    show_window_and_execute_tests(
        RunOptions::default(),
        window_with_bin,
        |runner, (bin, _decoy)| {
            let paths = vec![PathBuf::from("c.stl")];
            runner.file_drag_hover_by_name("bin", &DragOpts::default(), &paths);
            assert!(runner.app().file_drag_active());
            runner.file_drag_leave();
            assert!(!runner.app().file_drag_active());
            match file_events(bin).as_slice() {
                [Event::FileDragHover { pos, paths: got }, Event::FileDragLeave] => {
                    assert_eq!(*pos, Point::new(40.0, 10.0));
                    assert_eq!(*got, paths);
                }
                other => panic!("a hover then a leave, got {other:?}"),
            }

            // A hover ended by a drop: the leave comes first, as for a real
            // drop.
            runner.file_drag_hover_by_name("bin", &DragOpts::default(), &paths);
            runner.drop_files_by_name("bin", &DragOpts::default(), &paths);
            let kinds: Vec<&str> = file_events(bin)
                .iter()
                .skip(2)
                .map(|event| match event {
                    Event::FileDragHover { .. } => "hover",
                    Event::FileDragLeave => "leave",
                    Event::FileDropped { .. } => "drop",
                    _ => "other",
                })
                .collect();
            assert_eq!(kinds, ["hover", "leave", "drop"]);
            runner.mark_test_complete();
        },
    )
    .expect("the hover and leave arrive");
}
