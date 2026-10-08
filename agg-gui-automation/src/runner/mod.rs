//! `AutomationRunner`: what a test body drives — the port of agg-sharp
//! `GuiAutomation/AutomationRunner.cs`. This file holds its configuration and
//! run control; `waits.rs` the frame-pumping waits (`delay`, `wait_for`,
//! `assert`, ...) and `named.rs` the name lookups and the waits that poll
//! them, `pointer.rs` the pointer gestures (stepped moves, clicks),
//! `drag.rs` the drags and drops built on them, and `keyboard.rs` typing
//! and held modifiers.
//!
//! A runner is created by [`crate::execute::show_window_and_execute_tests`]
//! on the run's own UI thread and handed to the test body. It owns the
//! window's [`HeadlessDriver`], so every runner call runs on the UI thread and
//! pumps frames itself. When the run's wall-clock budget expires the caller
//! cancels the run; the body's next runner call then panics with
//! [`TEST_TIMED_OUT`], which unwinds a body that was stuck between calls.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agg_gui::{App, UnderMouseState};

use crate::driver::{HeadlessDriver, UiDriver};
use crate::input::{InputMethod, SimulatedInput};
use crate::tree_query::WidgetHandle;

mod drag;
mod keyboard;
mod named;
mod pointer;
mod scroll;
mod waits;

pub use drag::{DragDropOpts, DragOpts};
pub use keyboard::{ModifierKeys, CLOSE_CHORD};
pub use named::{WaitOpts, WidgetPredicate, DEFAULT_WIDGET_WAIT_SECONDS};
pub use pointer::{cubic_out, mouse_move_steps, ClickOpts, ClickOrigin};
pub use waits::{
    DEFAULT_CHECK_INTERVAL_MILLISECONDS, DEFAULT_CONDITION_WAIT_SECONDS, DEFAULT_DELAY_SECONDS,
    DEFAULT_UI_WORK_WAIT_MILLISECONDS,
};

/// The panic message of a runner call made after its run timed out.
pub const TEST_TIMED_OUT: &str = "test timed out";

/// Per-runner settings. C# keeps some of these as process-wide statics; here
/// each run has its own copy, with C#'s defaults, so tests running in
/// parallel cannot change each other's settings.
#[derive(Clone, Debug, PartialEq)]
pub struct AutomationConfig {
    /// C# `MatchLimit`: the least-squares limit of an image match (50).
    pub match_limit: i64,
    /// C# `RequireTestCompletion`: fail a run whose body did not call
    /// [`AutomationRunner::mark_test_complete`] (true). Every automation test
    /// must call it as its last action; that is how a run proves it reached
    /// its final statement rather than leaving early through a silent wait
    /// timeout or an accidental return. Wrappers that call it for the test
    /// (MatterCAD's `RunTest`) leave this on.
    pub require_test_completion: bool,
    /// C# `TimeToMoveMouse`: the longest one mouse move may take, in seconds
    /// (0.1). A ceiling, not a target: each step waits only until the UI has
    /// taken the move.
    pub time_to_move_mouse: f64,
    /// C# `MouseMoveSteps`: how many intermediate positions a move is broken
    /// into (5). Fixed rather than derived from `time_to_move_mouse`, because
    /// hover, drag tracking and tooltips key off seeing the pointer travel.
    pub mouse_move_steps: u32,
    /// C# `UpDelaySeconds`: the longest a simulated button may be held before
    /// it is released, in seconds (0.1); also a ceiling, not a target.
    pub up_delay: f64,
}

impl Default for AutomationConfig {
    fn default() -> Self {
        Self {
            match_limit: 50,
            require_test_completion: true,
            time_to_move_mouse: 0.1,
            mouse_move_steps: 5,
            up_delay: 0.1,
        }
    }
}

/// The handle a test body drives its window through; see the module docs.
pub struct AutomationRunner {
    /// This run's settings; a body may change them (C# sets
    /// `testRunner.RequireTestCompletion = false` the same way).
    pub config: AutomationConfig,
    driver: HeadlessDriver,
    /// Where pointer input goes (C# `inputSystem`): the forwarder, unless a
    /// test replaced it.
    input: Box<dyn InputMethod>,
    test_was_completed: bool,
    cancel: Arc<AtomicBool>,
}

impl AutomationRunner {
    /// A runner over a window that has come up. `cancel` is the run's
    /// timeout flag, set by the watching thread.
    pub(crate) fn new(
        driver: HeadlessDriver,
        config: AutomationConfig,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            config,
            driver,
            input: Box::new(SimulatedInput::new()),
            test_was_completed: false,
            cancel,
        }
    }

    /// Unwind the body if its run has timed out. Every public runner call
    /// starts with this, so a body that overstayed its budget stops at its
    /// next call instead of driving a window its caller has given up on.
    pub(crate) fn check_not_timed_out(&self) {
        if self.cancel.load(Ordering::SeqCst) {
            panic!("{TEST_TIMED_OUT}");
        }
    }

    /// C# `MarkTestComplete`: the body reached its final statement. Must be
    /// the last call of every automation test body (see
    /// [`AutomationConfig::require_test_completion`]).
    pub fn mark_test_complete(&mut self) {
        self.check_not_timed_out();
        self.test_was_completed = true;
    }

    /// C# `TestWasCompleted`: whether the body called
    /// [`mark_test_complete`](Self::mark_test_complete).
    pub fn test_was_completed(&self) -> bool {
        self.test_was_completed
    }

    /// The window's driver (frames, the framebuffer, the input forwarder).
    pub fn driver(&self) -> &HeadlessDriver {
        self.check_not_timed_out();
        &self.driver
    }

    /// The window's driver, mutably.
    pub fn driver_mut(&mut self) -> &mut HeadlessDriver {
        self.check_not_timed_out();
        &mut self.driver
    }

    /// The app under test (raw `App` entry points, as C#'s tests call
    /// `testWindow.OnMouseDown` directly).
    pub fn app(&self) -> &App {
        self.check_not_timed_out();
        self.driver.app()
    }

    /// The app under test, mutably.
    pub fn app_mut(&mut self) -> &mut App {
        self.check_not_timed_out();
        self.driver.app_mut()
    }

    /// C# `widget.UnderMouseState` for a found widget (see
    /// [`crate::pointer_state`]).
    pub fn under_mouse_state(&self, widget: &WidgetHandle) -> UnderMouseState {
        crate::pointer_state::under_mouse_state(self.app(), widget)
    }
}
