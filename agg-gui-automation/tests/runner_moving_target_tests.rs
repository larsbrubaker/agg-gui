//! Rust-only tests of a click by name on a named target that moves while the
//! pointer travels to it (`runner/pointer.rs`'s per-step re-aim through
//! `AutomationRunner::refind_target`): a part inside a 3D view that the
//! camera is still framing. The click must land where the target is when the
//! pointer arrives, not where it was when the move began.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::layout_props::WidgetBase;
use agg_gui::{clock, MouseButton, NamedTarget, Point, Rect, Size, Widget};
use agg_gui_automation::{show_window_and_execute_tests, AutomationWindow, RunOptions};

/// The name both targets answer to.
const PART: &str = "Moving Part";

/// Where the moving target starts and stops (host-local x of its left edge),
/// and how fast it travels: it reaches `END_X` 30 ms after it is first
/// asked for, well inside the click's five paced 10 ms steps.
const START_X: f64 = 10.0;
const END_X: f64 = 160.0;
const PX_PER_MS: f64 = 5.0;
const PART_SIZE: f64 = 30.0;
const PART_Y: f64 = 80.0;

/// A window-filling widget reporting two targets named [`PART`]: one that
/// slides right with the clock from the first time it is asked for, carrying
/// a shared object, and a still decoy with an object of its own (so the
/// re-aim must tell them apart by object, as neither a rectangle nor a lone
/// candidate identifies the moving one). It logs where presses land.
struct SlidingTargets {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    moving_object: Rc<dyn Any>,
    decoy_object: Rc<dyn Any>,
    started: Cell<Option<Instant>>,
    presses: Rc<RefCell<Vec<(Point, MouseButton)>>>,
}

impl SlidingTargets {
    fn moving_x(&self) -> f64 {
        let started = self.started.get().unwrap_or_else(|| {
            let now = clock::now();
            self.started.set(Some(now));
            now
        });
        let elapsed_ms = clock::since(started).as_secs_f64() * 1000.0;
        (START_X + elapsed_ms * PX_PER_MS).min(END_X)
    }
}

impl Widget for SlidingTargets {
    fn type_name(&self) -> &'static str {
        "SlidingTargets"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { pos, button, .. } => {
                self.presses.borrow_mut().push((*pos, *button));
                EventResult::Consumed
            }
            Event::MouseUp { .. } => EventResult::Consumed,
            _ => EventResult::Ignored,
        }
    }
    fn find_named_targets(&self, name: &str, out: &mut Vec<NamedTarget>) {
        if name != PART {
            return;
        }
        let moving = Rect::new(self.moving_x(), PART_Y, PART_SIZE, PART_SIZE);
        out.push(NamedTarget::new(moving).with_object(Rc::clone(&self.moving_object)));
        let decoy = Rect::new(250.0, PART_Y, PART_SIZE, PART_SIZE);
        out.push(NamedTarget::new(decoy).with_object(Rc::clone(&self.decoy_object)));
    }
}

#[test]
fn rust_only_click_by_name_follows_a_named_target_that_moves_during_the_move() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let presses: Rc<RefCell<Vec<(Point, MouseButton)>>> = Rc::default();
            let mut window = AutomationWindow::new(300.0, 200.0);
            window.add_child(Box::new(SlidingTargets {
                bounds: Rect::new(0.0, 0.0, 300.0, 200.0),
                base: WidgetBase::new(),
                children: Vec::new(),
                moving_object: Rc::new("moving".to_string()),
                decoy_object: Rc::new("decoy".to_string()),
                started: Cell::new(None),
                presses: Rc::clone(&presses),
            }));
            (window, presses)
        },
        |runner, presses| {
            // Found while it is still at its start, so the hit aims there.
            let hit = runner.find_now(PART).expect("the moving part is on show");
            assert_eq!(hit.offset_hint.x, START_X + PART_SIZE / 2.0);

            runner.click_by_name(PART);

            let presses = presses.borrow();
            assert_eq!(presses.len(), 1, "one press: {presses:?}");
            let (at, button) = presses[0];
            assert_eq!(button, MouseButton::Left);
            assert!(
                at.x >= END_X && at.x <= END_X + PART_SIZE,
                "the press lands on the part where it came to rest \
                 (x {END_X}..{}), not where it was: {at:?}",
                END_X + PART_SIZE
            );
            assert!(at.y >= PART_Y && at.y <= PART_Y + PART_SIZE, "{at:?}");
            runner.mark_test_complete();
        },
    )
    .expect("a click follows a moving named target");
}
