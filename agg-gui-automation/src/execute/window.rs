//! `AutomationWindow`: the window a run shows — C#'s `SystemWindow` as handed
//! to `AutomationRunner.ShowWindowAndExecuteTests`, with its `Load` event.
//!
//! It is a description, built on the run's own UI thread by the `build`
//! closure of [`super::show_window_and_execute_tests`]: a root widget, a
//! logical size, an `on_load` hook, and the close hooks the run's close phase
//! (`close.rs`) answers to: `on_close_requested` (C# `ShouldClose`, which can
//! veto) and `on_closed` (C# `Closed`, after which `HasBeenClosed` is true).
//! Bringing it up (creating the [`HeadlessDriver`], painting the first frame,
//! firing `on_load`) is the run's bring-up phase, timed by its own budget
//! rather than the test's.

use agg_gui::{App, Widget};

use crate::driver::{ClockPolicy, FrameKind, HeadlessDriver, UiDriver};
use crate::probe::ProbeWidget;

/// The name of the root widget [`AutomationWindow::new`] creates.
pub const AUTOMATION_WINDOW_ROOT_NAME: &str = "AutomationWindow";

type LoadHook = Box<dyn FnOnce(&mut App)>;
type CloseRequestedHook = Box<dyn FnMut(&mut App) -> bool>;
type ClosedHook = Box<dyn FnOnce(&mut App)>;

/// The window's close hooks, handed from bring-up to the close phase.
#[derive(Default)]
pub(crate) struct CloseHooks {
    pub(crate) on_close_requested: Option<CloseRequestedHook>,
    pub(crate) on_closed: Option<ClosedHook>,
}

/// A window for one automation run; see the module docs.
pub struct AutomationWindow {
    root: Box<dyn Widget>,
    width: f64,
    height: f64,
    on_load: Option<LoadHook>,
    close_hooks: CloseHooks,
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
            close_hooks: CloseHooks::default(),
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

    /// C# `ShouldClose`: asked each time the window is asked to close;
    /// `true` lets it close, `false` vetoes the close (C# `e.Cancel = true`,
    /// the shape of a "save changes?" prompt). A window that keeps vetoing is
    /// force closed when the run's close budget runs out, and the run fails
    /// with [`AutomationError::CloseTimeout`](super::AutomationError::CloseTimeout).
    pub fn on_close_requested(mut self, hook: impl FnMut(&mut App) -> bool + 'static) -> Self {
        self.close_hooks.on_close_requested = Some(Box::new(hook));
        self
    }

    /// C# `Closed`: run `hook` once, when the window closes, whether it
    /// closed on request or was force closed; the tree is still alive.
    pub fn on_closed(mut self, hook: impl FnOnce(&mut App) + 'static) -> Self {
        self.close_hooks.on_closed = Some(Box::new(hook));
        self
    }

    /// The window's logical size.
    pub fn size(&self) -> (f64, f64) {
        (self.width, self.height)
    }

    /// Bring the window up on the calling thread: create its driver at the
    /// thread's device scale, paint the first frame, then fire `on_load`.
    /// Returns the driver and the close hooks for the close phase.
    pub(crate) fn bring_up(self, clock: ClockPolicy) -> (HeadlessDriver, CloseHooks) {
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
        (driver, self.close_hooks)
    }
}
