//! Rust-only tests of the runner's file drags and drops
//! (`runner/file_drop.rs`): a drag travels as file-drag hovers at the
//! runner's stepped positions with no mouse moves, so no widget enters mouse
//! hover; a drop by path or by contents lands on the named widget; and a
//! drag that leaves puts the pointer outside the window with nothing hovered.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use agg_gui::event::{Event, EventResult};
use agg_gui::{DroppedFileData, Point, Rect};
use agg_gui_automation::runner::mouse_move_steps;
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationWindow, ClickOpts, DragOpts, InputMethod, Keys,
    MouseAction, Point2D, ProbeLog, ProbeWidget, RunOptions, SimulatedInput, TypedKey,
};

/// A [`SimulatedInput`] that records every pointer position it is sent:
/// `moves` the ones the window sees as mouse moves, `recorded` the ones an
/// OS drag carries the pointer to without a move.
struct RecordingInput {
    inner: SimulatedInput,
    moves: Rc<RefCell<Vec<Point2D>>>,
    recorded: Rc<RefCell<Vec<Point2D>>>,
}

type PositionLog = Rc<RefCell<Vec<Point2D>>>;

/// Put a [`RecordingInput`] on `runner`; returns its move and recorded
/// position logs.
fn record_input(runner: &mut agg_gui_automation::AutomationRunner) -> (PositionLog, PositionLog) {
    let moves = PositionLog::default();
    let recorded = PositionLog::default();
    runner.set_input_method(Box::new(RecordingInput {
        inner: SimulatedInput::new(),
        moves: Rc::clone(&moves),
        recorded: Rc::clone(&recorded),
    }));
    (moves, recorded)
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

    fn record_cursor_position(&mut self, x: i32, y: i32) {
        self.recorded.borrow_mut().push(Point2D::new(x, y));
        self.inner.record_cursor_position(x, y);
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

/// The kinds of the file events in `log`, consecutive hovers counted once.
fn file_event_kinds(log: &ProbeLog) -> Vec<&'static str> {
    let mut kinds: Vec<&'static str> = file_events(log)
        .iter()
        .map(|event| match event {
            Event::FileDragHover { .. } => "hover",
            Event::FileDragLeave => "leave",
            Event::FileDropped { .. } => "drop",
            Event::FileDataDropped { .. } => "data drop",
            _ => "other",
        })
        .collect();
    kinds.dedup_by(|a, b| *a == "hover" && *b == "hover");
    kinds
}

#[test]
fn rust_only_drop_files_by_name_delivers_the_paths_at_the_target_after_a_stepped_move() {
    show_window_and_execute_tests(
        RunOptions::default(),
        window_with_bin,
        |runner, (bin, decoy)| {
            let (moves, recorded) = record_input(runner);
            let start = runner.current_mouse_position();
            let paths = vec![PathBuf::from("/parts/a.stl"), PathBuf::from("b.mcx")];
            runner.drop_files_by_name("bin", &DragOpts::default(), &paths);

            // The bin spans x 50..130, y 40..60 (Y-up) in a 200-high window:
            // its center (90, 50) is pointer (90, 150), reached in the
            // runner's eased steps, which the drag carries the pointer
            // through without a single mouse move.
            let end = Point2D::new(90, 150);
            assert_eq!(runner.current_mouse_position(), end);
            assert_eq!(
                *recorded.borrow(),
                mouse_move_steps(start, end, runner.config.mouse_move_steps)
            );
            assert!(moves.borrow().is_empty(), "no mouse moves");
            let drops: Vec<Event> = file_events(bin)
                .into_iter()
                .filter(|event| matches!(event, Event::FileDropped { .. }))
                .collect();
            match drops.as_slice() {
                [Event::FileDropped { pos, paths: got }] => {
                    assert_eq!(*pos, Point::new(40.0, 10.0));
                    assert_eq!(*got, paths);
                }
                other => panic!("one drop on the bin, got {other:?}"),
            }
            // The hovers then the drop, which App ends the drag before.
            assert_eq!(file_event_kinds(bin), ["hover", "leave", "drop"]);
            assert!(!file_events(decoy)
                .iter()
                .any(|event| matches!(event, Event::FileDropped { .. })));
            assert!(!runner.app().file_drag_active());
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
            let (moves, _recorded) = record_input(runner);
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
            assert!(moves.borrow().is_empty(), "no mouse moves");
            // The browser's hovers name no files.
            for event in file_events(bin) {
                if let Event::FileDragHover { paths, .. } = event {
                    assert!(paths.is_empty());
                }
            }
            assert_eq!(file_event_kinds(bin), ["hover", "leave", "data drop"]);
            match file_events(bin).last() {
                Some(Event::FileDataDropped { pos, files }) => {
                    assert_eq!(*pos, Point::new(10.0, 5.0));
                    assert_eq!(files.len(), 1);
                    assert_eq!(files[0].name, "note.txt");
                    assert_eq!(&*files[0].bytes, b"hello");
                }
                other => panic!("a data drop on the bin, got {other:?}"),
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
            match file_events(bin).last() {
                Some(Event::FileDragHover { pos, paths: got }) => {
                    assert_eq!(*pos, Point::new(40.0, 10.0));
                    assert_eq!(*got, paths);
                }
                other => panic!("the last hover at the bin's center, got {other:?}"),
            }
            runner.file_drag_leave();
            assert!(!runner.app().file_drag_active());
            assert_eq!(file_event_kinds(bin), ["hover", "leave"]);

            // The drag left through the bottom edge, the nearest to pointer
            // (90, 150) in the 300 × 200 window, and the pointer is past it.
            assert_eq!(runner.current_mouse_position(), Point2D::new(90, 201));
            assert_eq!(runner.app().hovered_chain(), None);

            // A hover ended by a drop: the leave comes first, as for a real
            // drop.
            runner.file_drag_hover_by_name("bin", &DragOpts::default(), &paths);
            runner.drop_files_by_name("bin", &DragOpts::default(), &paths);
            assert_eq!(
                file_event_kinds(bin),
                ["hover", "leave", "hover", "leave", "drop"]
            );
            runner.mark_test_complete();
        },
    )
    .expect("the hover and leave arrive");
}

/// A 300 × 200 window filled by `field`, which takes every file event,
/// holding two widgets that take none: `spot` at (100, 50) sized 60 × 40
/// and `other` at (10, 150) sized 40 × 30 (Y-up).
fn window_with_spot() -> (AutomationWindow, (ProbeLog, ProbeLog, ProbeLog)) {
    let mut window = AutomationWindow::new(300.0, 200.0);
    let spot = ProbeWidget::new("spot").with_bounds(Rect::new(100.0, 50.0, 60.0, 40.0));
    let other = ProbeWidget::new("other").with_bounds(Rect::new(10.0, 150.0, 40.0, 30.0));
    let logs = (spot.log(), other.log());
    let field = ProbeWidget::new("field")
        .with_bounds(Rect::new(0.0, 0.0, 300.0, 200.0))
        .on_event_with(|event| match event {
            Event::FileDragHover { .. } => EventResult::Consumed,
            _ => EventResult::Ignored,
        })
        .with_child(Box::new(spot))
        .with_child(Box::new(other));
    let field_log = field.log();
    window.add_child(Box::new(field));
    (window, (field_log, logs.0, logs.1))
}

fn is_mouse_hover_event(event: &Event) -> bool {
    matches!(
        event,
        Event::MouseEnter | Event::MouseMove { .. } | Event::MouseLeave
    )
}

#[test]
fn rust_only_a_file_drag_is_file_drag_hovers_and_never_hovers_the_mouse() {
    show_window_and_execute_tests(
        RunOptions::default(),
        window_with_spot,
        |runner, (field, spot, other)| {
            // The mouse rests over `other`, which is hovered.
            let (moves, recorded) = record_input(runner);
            assert!(runner.move_to_by_name("other", &ClickOpts::default()));
            assert!(other
                .borrow()
                .iter()
                .any(|event| matches!(event, Event::MouseEnter)));
            let start = runner.current_mouse_position();
            assert_eq!(start, Point2D::new(30, 35));
            let moves_before = moves.borrow().len();
            let (spot_before, other_before, field_before) = (
                spot.borrow().len(),
                other.borrow().len(),
                field.borrow().len(),
            );

            let paths = vec![PathBuf::from("part.stl")];
            runner.file_drag_hover_by_name("spot", &DragOpts::default(), &paths);

            // The spot's center (130, 70) is pointer (130, 130). The drag
            // got there in the runner's steps, each a hover at that point
            // (the field's coordinates are the window's, Y-up), and the
            // window saw no mouse move.
            let end = Point2D::new(130, 130);
            let steps = mouse_move_steps(start, end, runner.config.mouse_move_steps);
            assert_eq!(*recorded.borrow(), steps);
            assert_eq!(moves.borrow().len(), moves_before, "no mouse moves");
            let hovers: Vec<Point> = field.borrow()[field_before..]
                .iter()
                .filter_map(|event| match event {
                    Event::FileDragHover { pos, .. } => Some(*pos),
                    _ => None,
                })
                .collect();
            let expected: Vec<Point> = steps
                .iter()
                .map(|p| Point::new(f64::from(p.x), 200.0 - f64::from(p.y)))
                .collect();
            assert_eq!(hovers, expected);

            // The pointer left the window to pick the files up, so `other`
            // lost its hover, and the drag over the spot hovers nothing.
            assert!(other.borrow()[other_before..]
                .iter()
                .any(|event| matches!(event, Event::MouseLeave)));
            assert!(
                !spot.borrow()[spot_before..]
                    .iter()
                    .any(is_mouse_hover_event),
                "the spot saw no mouse hover during the file drag"
            );
            assert_eq!(runner.app().hovered_chain(), None);

            // The drag leaves: nothing is hovered and the pointer is outside.
            runner.file_drag_leave();
            assert_eq!(runner.app().hovered_chain(), None);
            assert!(!spot.borrow()[spot_before..]
                .iter()
                .any(is_mouse_hover_event));
            let (width, height) = runner.driver().size_px();
            let p = runner.current_mouse_position();
            assert!(
                p.x < 0 || p.y < 0 || p.x > width as i32 || p.y > height as i32,
                "the pointer is outside the window, got {p:?}"
            );

            // A mouse move afterwards starts from there and hovers as ever.
            assert!(runner.move_to_by_name("spot", &ClickOpts::default()));
            assert_eq!(moves.borrow().get(moves_before), Some(&p));
            assert!(spot.borrow()[spot_before..]
                .iter()
                .any(|event| matches!(event, Event::MouseEnter)));
            runner.mark_test_complete();
        },
    )
    .expect("the file drag hovers no widget");
}
