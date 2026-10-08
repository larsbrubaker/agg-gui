//! `HeadlessDriver`: an `App` run with no window and no GPU — the frames the
//! shells run, painted into a software framebuffer, on a virtual clock by
//! default.
//!
//! One pumped frame is a shell's reactive tick: advance the clock by
//! [`FRAME_INTERVAL`] (virtual clock only), then
//! [`LayoutTracker::tick`](agg_gui::frame_policy::LayoutTracker::tick) — drain
//! the thread's `ui_thread` queue, and if the frame is forced or the app wants
//! to draw, lay out when the layout key changed and paint. Input goes through
//! [`InputForwarder`], the bookkeeping both shells feed their OS events
//! through, so simulated and real input run the same code.
//!
//! Creating a driver makes the calling thread a UI thread
//! (`ui_thread::mark_current_thread_as_ui_thread`): its queue, clock and
//! wakeups are its own, so tests on separate threads do not interfere.

use agg_gui::clock::{self, ClockGuard};
use agg_gui::frame_policy::{LayoutTracker, TickMode};
use agg_gui::shell_input::{ForwarderEvent, InputForwarder};
use agg_gui::{App, Framebuffer, GfxCtx, Size, Widget};

use super::{ClockPolicy, FrameKind, UiDriver, FRAME_INTERVAL};

/// An `App` with a software framebuffer, a frame policy and an input
/// forwarder; see the module docs.
pub struct HeadlessDriver {
    app: App,
    tracker: LayoutTracker,
    forwarder: InputForwarder,
    framebuffer: Framebuffer,
    clock: ClockPolicy,
    frames: u64,
    painted: u64,
    close_requested: bool,
    // Last, so the app (and anything it reads the clock for while dropping)
    // goes first and the thread's clock is restored after.
    _clock: Option<ClockGuard>,
}

impl HeadlessDriver {
    /// Drive `root` in a `width × height` physical-pixel window on the
    /// virtual clock.
    pub fn new(root: Box<dyn Widget>, width: u32, height: u32) -> Self {
        Self::with_clock(root, width, height, ClockPolicy::Virtual)
    }

    /// [`new`](Self::new) on the given clock.
    pub fn with_clock(root: Box<dyn Widget>, width: u32, height: u32, clock: ClockPolicy) -> Self {
        agg_gui::ui_thread::mark_current_thread_as_ui_thread();
        let guard = match clock {
            ClockPolicy::Virtual => Some(clock::scoped_virtual(None)),
            ClockPolicy::Real => None,
        };
        Self {
            app: App::new(root),
            tracker: LayoutTracker::new(),
            forwarder: InputForwarder::new(),
            framebuffer: Framebuffer::new(width.max(1), height.max(1)),
            clock,
            frames: 0,
            painted: 0,
            close_requested: false,
            _clock: guard,
        }
    }

    /// The clock this driver runs on.
    pub fn clock_policy(&self) -> ClockPolicy {
        self.clock
    }

    /// The window size in physical pixels.
    pub fn size_px(&self) -> (u32, u32) {
        (self.framebuffer.width(), self.framebuffer.height())
    }

    fn viewport_px(&self) -> Size {
        Size::new(
            f64::from(self.framebuffer.width()),
            f64::from(self.framebuffer.height()),
        )
    }

    /// Resize the window (physical pixels); the next frame lays out again.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.framebuffer.resize(width.max(1), height.max(1));
    }

    /// Lay the app out now if its layout key changed or a layout was
    /// requested; returns whether it did. Input entry points hit-test against
    /// the last layout, so call this before sending input to a fresh tree.
    pub fn layout_if_needed(&mut self) -> bool {
        let viewport = self.viewport_px();
        self.tracker.layout_if_needed(&mut self.app, viewport)
    }

    /// Deliver a simulated input event through the forwarder.
    pub fn send(&mut self, event: ForwarderEvent) {
        self.forwarder.simulated(&mut self.app, event);
    }

    /// The input bookkeeping (cursor, held buttons and modifiers, clicks).
    pub fn forwarder(&self) -> &InputForwarder {
        &self.forwarder
    }

    /// The input bookkeeping, mutably (click policy, real-input gate).
    pub fn forwarder_mut(&mut self) -> &mut InputForwarder {
        &mut self.forwarder
    }

    /// Whether something asked the window to close
    /// ([`UiDriver::request_close`], e.g. typing `%{F4}`).
    pub fn close_requested(&self) -> bool {
        self.close_requested
    }

    /// Frames pumped so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Frames that painted so far.
    pub fn frames_painted(&self) -> u64 {
        self.painted
    }

    /// The root of the widget tree (for [`crate::tree_query`]).
    pub fn root(&self) -> &dyn Widget {
        self.app.root()
    }

    /// The root of the widget tree, mutably.
    pub fn root_mut(&mut self) -> &mut dyn Widget {
        self.app.root_mut()
    }
}

impl UiDriver for HeadlessDriver {
    fn app(&self) -> &App {
        &self.app
    }

    fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    fn pump(&mut self, kind: FrameKind) -> bool {
        if self.clock == ClockPolicy::Virtual {
            clock::advance(FRAME_INTERVAL);
        }
        let mode = match kind {
            FrameKind::Reactive => TickMode::Reactive,
            FrameKind::Forced => TickMode::Forced,
        };
        let viewport = self.viewport_px();
        let framebuffer = &mut self.framebuffer;
        let painted = self.tracker.tick(&mut self.app, viewport, mode, |app| {
            // A shell presents a fresh surface each frame; start from clear.
            framebuffer.pixels_mut().fill(0);
            let mut ctx = GfxCtx::new(framebuffer);
            app.paint(&mut ctx);
        });
        self.frames += 1;
        if painted {
            self.painted += 1;
        }
        painted
    }

    fn logical_size(&self) -> Size {
        let scale = agg_gui::ux_scale::effective_scale().max(1e-6);
        let px = self.viewport_px();
        Size::new(px.width / scale, px.height / scale)
    }

    fn current_screen(&self) -> &Framebuffer {
        &self.framebuffer
    }

    fn size_px(&self) -> (u32, u32) {
        HeadlessDriver::size_px(self)
    }

    fn send(&mut self, event: ForwarderEvent) {
        HeadlessDriver::send(self, event);
    }

    fn request_close(&mut self) {
        self.close_requested = true;
    }
}
