//! `AutomationWindow`: the window a run shows — C#'s `SystemWindow` as handed
//! to `AutomationRunner.ShowWindowAndExecuteTests`, with its `Load` event.
//!
//! It is a description, built on the run's own UI thread by the `build`
//! closure of [`super::show_window_and_execute_tests`]: a root widget, a
//! logical size and an `on_load` hook. Bringing it up (creating the
//! [`HeadlessDriver`], painting the first frame, firing `on_load`) is the
//! run's bring-up phase, timed by its own budget rather than the test's.

use agg_gui::{App, Widget};

use crate::driver::{ClockPolicy, FrameKind, HeadlessDriver, UiDriver};
use crate::probe::ProbeWidget;

/// The name of the root widget [`AutomationWindow::new`] creates.
pub const AUTOMATION_WINDOW_ROOT_NAME: &str = "AutomationWindow";

type LoadHook = Box<dyn FnOnce(&mut App)>;

/// A window for one automation run; see the module docs.
pub struct AutomationWindow {
    root: Box<dyn Widget>,
    width: f64,
    height: f64,
    on_load: Option<LoadHook>,
}

impl AutomationWindow {
    /// A `width × height` (logical) window whose root is a [`ProbeWidget`]
    /// that keeps its children where they are placed (C#
    /// `new SystemWindow(width, height)`).
    pub fn new(width: f64, height: f64) -> Self {
        Self::with_root(
            Box::new(ProbeWidget::new(AUTOMATION_WINDOW_ROOT_NAME)),
            width,
            height,
        )
    }

    /// A `width × height` (logical) window around an application's own root.
    pub fn with_root(root: Box<dyn Widget>, width: f64, height: f64) -> Self {
        Self {
            root,
            width,
            height,
            on_load: None,
        }
    }

    /// C# `AddChild`: add `child` to the root.
    pub fn add_child(&mut self, child: Box<dyn Widget>) {
        self.root.children_mut().push(child);
    }

    /// C# `Load`: run `hook` once the window has painted its first frame,
    /// before the test body starts. Time spent here is bring-up, not test.
    pub fn on_load(mut self, hook: impl FnOnce(&mut App) + 'static) -> Self {
        self.on_load = Some(Box::new(hook));
        self
    }

    /// The window's logical size.
    pub fn size(&self) -> (f64, f64) {
        (self.width, self.height)
    }

    /// Bring the window up on the calling thread: create its driver at the
    /// thread's device scale, paint the first frame, then fire `on_load`.
    pub(crate) fn bring_up(self, clock: ClockPolicy) -> HeadlessDriver {
        let scale = agg_gui::ux_scale::effective_scale();
        let mut driver = HeadlessDriver::with_clock(
            self.root,
            (self.width * scale).round() as u32,
            (self.height * scale).round() as u32,
            clock,
        );
        driver.pump(FrameKind::Forced);
        if let Some(hook) = self.on_load {
            hook(driver.app_mut());
        }
        driver
    }
}
