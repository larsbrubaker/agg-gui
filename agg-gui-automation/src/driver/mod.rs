//! Drivers: what owns the `App` under test and runs its frames. C#'s
//! automation runs a real `SystemWindow` whose message loop owns a thread; here
//! the test thread *is* the UI thread and every runner call pumps frames
//! itself (design: `docs/design/gui-automation.md`, section 5).
//!
//! - [`HeadlessDriver`] — no window, no GPU: a software framebuffer and, by
//!   default, a virtual clock that advances one [`FRAME_INTERVAL`] per frame.
//! - [`HeadlessWindow`] — a [`HeadlessDriver`] around a root
//!   [`ProbeWidget`](crate::probe::ProbeWidget): the "SystemWindow" for ported
//!   tests that build widgets and send them input without the runner, with
//!   C#'s Y-up window coordinates.
//!
//! The live (winit) driver is a later slice.

mod headless;
mod window;

use std::time::Duration;

pub use headless::HeadlessDriver;
pub use window::HeadlessWindow;

/// One frame's worth of UI time: the RunOnIdle tick C# cites (10 ms).
pub const FRAME_INTERVAL: Duration = Duration::from_millis(10);

/// Which clock a headless driver runs the UI thread on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClockPolicy {
    /// The thread's [`agg_gui::clock`] is virtual and each pumped frame
    /// advances it by the frame interval: deterministic, no sleeping.
    #[default]
    Virtual,
    /// The real clock, for tests whose background workers run in real time.
    Real,
}

/// Whether a pumped frame paints regardless of the app's draw request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    /// Paint only if the app wants to draw (an idle shell tick).
    Reactive,
    /// Lay out (when due) and paint unconditionally (C# `WaitforDraw`).
    Forced,
}

/// What every driver offers the runner: the app, its frames, its pixels.
pub trait UiDriver {
    /// The app under test.
    fn app(&self) -> &agg_gui::App;
    /// The app under test, mutably (raw `App::on_*` entry points).
    fn app_mut(&mut self) -> &mut agg_gui::App;
    /// Run one frame; returns whether it painted.
    fn pump(&mut self, kind: FrameKind) -> bool;
    /// The window's size in logical units (what the root is laid out at).
    fn logical_size(&self) -> agg_gui::Size;
    /// The last painted frame.
    fn current_screen(&self) -> &agg_gui::Framebuffer;
    /// The window's size in physical pixels (the pointer space's extent).
    fn size_px(&self) -> (u32, u32);
    /// Deliver a simulated input event (physical pixels, Y-down) through the
    /// window's input forwarder.
    fn send(&mut self, event: agg_gui::shell_input::ForwarderEvent);
}
