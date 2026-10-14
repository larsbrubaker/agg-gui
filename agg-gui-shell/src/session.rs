//! [`ShellSession`]: the shell's window, GPU and event loop, pumped by the
//! caller instead of owning the thread.
//!
//! [`crate::run`] hands the thread to winit until the window closes. A test
//! driver (agg-gui-automation's `LiveDriver`) needs the opposite: it runs the
//! test body on the UI thread and pumps the window's events itself between
//! steps. `ShellSession` does `run`'s bring-up (`run::bring_up`) and then
//! pumps the very same [`crate::shell_loop::ShellLoop`] through winit's
//! `pump_events` (Windows, macOS, Linux), so every real event takes the arms
//! `run` uses.
//!
//! It also carries the gates an automation run needs (agg-sharp's
//! `RealInputIgnored` and `DesktopDeactivationIgnored`): real input and
//! window activation can be switched off while simulated input, sent through
//! the same [`InputForwarder`], always gets through.
//!
//! Unlike `run`, a session leaves agg-gui on its in-process clipboard, so a
//! test never touches the user's clipboard. winit allows one event loop per
//! process, created on the main thread on macOS: one session per process.

#![allow(deprecated)] // winit 0.30's closure `pump_events`, as `run` uses `EventLoop::run`

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use agg_gui::shell_input::{ForwarderEvent, InputForwarder};
use agg_gui::App;
use winit::event::WindowEvent;
use winit::event_loop::EventLoop;
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::window::Window;

use crate::host::{ExitAction, ShellHost};
use crate::run::{bring_up, BroughtUp};
use crate::shell_init::ShellInit;
use crate::shell_loop::ShellLoop;
use crate::{ShellConfig, ShellError};

/// An event queued for the next [`ShellSession::pump`], delivered as the OS
/// would deliver it.
enum Queued {
    /// Through the shell's winit arm for it.
    Window(WindowEvent),
    /// At the point a winit arm hands its event on (the real-input gate).
    Platform(ForwarderEvent),
}

/// A shell window whose event loop the caller pumps; see the module docs.
pub struct ShellSession<H: ShellHost> {
    event_loop: EventLoop<()>,
    shell: ShellLoop<H>,
    exit: Rc<Cell<Option<ExitAction>>>,
    error: Rc<RefCell<Option<ShellError>>>,
    queued: VecDeque<Queued>,
    exited: bool,
}

impl<H: ShellHost + 'static> ShellSession<H> {
    /// Open the window, bring up wgpu, build the app and paint its first
    /// frame, exactly as [`crate::run`] does, then return without running
    /// the loop. Marks the calling thread as the UI thread.
    pub fn open<B>(config: ShellConfig, build: B) -> Result<Self, ShellError>
    where
        B: FnOnce(&ShellInit<'_>) -> Result<(App, H), ShellError>,
    {
        agg_gui::ui_thread::mark_current_thread_as_ui_thread();
        let BroughtUp {
            event_loop,
            shell,
            exit,
            error,
        } = bring_up(config, build)?;
        Ok(Self {
            event_loop,
            shell,
            exit,
            error,
            queued: VecDeque::new(),
            exited: false,
        })
    }
}

impl<H: ShellHost> ShellSession<H> {
    /// Run the event loop once: the queued events first, then whatever the
    /// OS has, waiting at most `timeout` for something to arrive (`None`
    /// waits). Returns `false` once the loop has exited (a close, a failure),
    /// after which pumping does nothing.
    pub fn pump(&mut self, timeout: Option<Duration>) -> bool {
        if self.exited {
            return false;
        }
        let shell = &mut self.shell;
        let queued = &mut self.queued;
        let status = self.event_loop.pump_events(timeout, |event, elwt| {
            while let Some(next) = queued.pop_front() {
                match next {
                    Queued::Window(event) => shell.window_event(event, elwt),
                    Queued::Platform(event) => shell.forward(event),
                }
            }
            shell.handle(event, elwt);
        });
        if let PumpStatus::Exit(_) = status {
            self.exited = true;
        }
        !self.exited
    }

    /// Whether the loop has exited.
    pub fn has_exited(&self) -> bool {
        self.exited
    }

    /// The first error that ended the loop (a lost surface, a failed GPU
    /// rebuild), taken.
    pub fn take_error(&mut self) -> Option<ShellError> {
        self.error.borrow_mut().take()
    }

    /// The app.
    pub fn app(&self) -> &App {
        &self.shell.app
    }

    /// The app, mutably.
    pub fn app_mut(&mut self) -> &mut App {
        &mut self.shell.app
    }

    /// The host.
    pub fn host(&self) -> &H {
        &self.shell.host
    }

    /// The host, mutably.
    pub fn host_mut(&mut self) -> &mut H {
        &mut self.shell.host
    }

    /// The OS window.
    pub fn window(&self) -> &Window {
        &self.shell.window
    }

    /// The window's input bookkeeping (cursor, held buttons and modifiers,
    /// click counting, the real-input gate).
    pub fn forwarder(&self) -> &InputForwarder {
        &self.shell.input
    }

    /// Deliver a simulated input event now, through the forwarder the winit
    /// arms feed. Never gated.
    pub fn send_simulated(&mut self, event: ForwarderEvent) {
        self.shell.input_since_frame = true;
        self.shell.input.simulated(&mut self.shell.app, event);
    }

    /// Queue a window event for the start of the next [`pump`](Self::pump),
    /// where it takes the shell's own winit arm, as an event from the OS
    /// does (a real mouse move, a cursor leave, a focus change, a close).
    pub fn queue_window_event(&mut self, event: WindowEvent) {
        self.queued.push_back(Queued::Window(event));
    }

    /// Queue OS input for the start of the next [`pump`](Self::pump), at the
    /// point the shell's winit arms hand their events on, the real-input
    /// gate included. For what winit cannot build outside itself: a key
    /// press (`winit::event::KeyEvent` has private fields) becomes the
    /// `ForwarderEvent` its `KeyboardInput` arm would forward.
    pub fn queue_platform_input(&mut self, event: ForwarderEvent) {
        self.queued.push_back(Queued::Platform(event));
    }

    /// Let real input through, or drop it (agg-sharp's `RealInputIgnored`):
    /// gates every input arm; the window still paints, resizes and closes.
    pub fn set_platform_input_enabled(&mut self, enabled: bool) {
        self.shell.input.set_platform_input_enabled(enabled);
    }

    /// Whether real input reaches the app.
    pub fn platform_input_enabled(&self) -> bool {
        self.shell.input.platform_input_enabled()
    }

    /// Let the window gaining or losing key status reach the app, or drop it
    /// (agg-sharp's `DesktopDeactivationIgnored`).
    pub fn set_platform_deactivation_enabled(&mut self, enabled: bool) {
        self.shell.platform_deactivation = enabled;
    }

    /// Whether window activation changes reach the app.
    pub fn platform_deactivation_enabled(&self) -> bool {
        self.shell.platform_deactivation
    }

    /// Ask the window to close as its close button does: the next pump runs
    /// the `CloseRequested` arm, so [`ShellHost::on_close_requested`] may
    /// veto it.
    pub fn request_close(&mut self) {
        self.queue_window_event(WindowEvent::CloseRequested);
    }

    /// Close the window at the next pump, with no veto.
    pub fn force_close(&mut self) {
        if self.exit.get().is_none() {
            self.exit.set(Some(ExitAction::Exit));
        }
    }

    /// Frames painted so far, the first (hidden-window) frame included.
    pub fn presented_frames(&self) -> u64 {
        self.shell.painter.frames_painted()
    }
}

impl<H: ShellHost> Drop for ShellSession<H> {
    /// The loop's final flush (bounds, [`ShellHost::on_exit`], the GPU
    /// drained within its budget), once, if the loop never exited.
    fn drop(&mut self) {
        self.shell.finish();
    }
}
