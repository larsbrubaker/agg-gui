//! `HeadlessDriver`: an `App` run with no window — the frames the shells run,
//! painted by a [`HeadlessPainter`] (a software framebuffer by default), on a
//! virtual clock by default.
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
//!
//! **Several drivers on one thread** (a restart test holding the old and the
//! new app) share that thread's queue and clock:
//! - The clock: drivers alive on one thread must use the same
//!   [`ClockPolicy`]; virtual ones share one virtual clock, which goes back to
//!   what the thread had before only when the last of them drops, in any
//!   order (`thread_clock.rs`).
//! - The UI-thread mark: a driver marks its thread but never unbinds it,
//!   because it cannot know whether the thread was a UI thread before it, or
//!   still serves another driver. Whoever owns the thread unbinds it
//!   (`ui_thread::unbind_current_thread`) once its last driver is gone and
//!   its queued work no longer matters — a test harness at the end of its
//!   test, so the main queue it may have adopted passes to the next test at
//!   once. [`crate::execute::show_window_and_execute_tests`] needs no unbind:
//!   its thread ends with the run.
//!
//! **Background work.** An application whose work finishes off the UI
//! thread (compute workers) tells the driver about it with
//! [`HeadlessDriver::set_background_busy`]; while the hook reports true the
//! driver is not idle ([`HeadlessDriver::is_idle`]), so the runner's idle
//! waits keep pumping until the results have landed.

use std::cell::OnceCell;

use agg_gui::clock;
use agg_gui::frame_policy::{LayoutTracker, TickMode};
use agg_gui::shell_input::{ForwarderEvent, InputForwarder};
use agg_gui::{App, Framebuffer, Size, Widget};

use super::painter::{HeadlessPainter, SoftwarePainter};
use super::thread_clock::ClockShare;
use super::{ClockPolicy, FrameKind, UiDriver, FRAME_INTERVAL};

/// Whether background work the app is waiting on is still running; see
/// [`HeadlessDriver::set_background_busy`].
pub type BackgroundBusy = Box<dyn Fn(&App) -> bool>;

/// An `App` with a painter, a frame policy and an input forwarder; see the
/// module docs.
pub struct HeadlessDriver {
    app: App,
    tracker: LayoutTracker,
    forwarder: InputForwarder,
    painter: Box<dyn HeadlessPainter>,
    /// The window size in physical pixels.
    width: u32,
    height: u32,
    /// The last frame of a painter that keeps no [`Framebuffer`] of its own,
    /// captured on first ask and dropped by the next painted frame.
    captured: OnceCell<Framebuffer>,
    background_busy: Option<BackgroundBusy>,
    clock: ClockPolicy,
    frames: u64,
    painted: u64,
    close_requested: bool,
    // Last, so the app (and anything it reads the clock for while dropping)
    // goes first and the thread's clock is restored after.
    _clock: ClockShare,
}

impl HeadlessDriver {
    /// Drive `root` in a `width × height` physical-pixel window on the
    /// virtual clock.
    pub fn new(root: Box<dyn Widget>, width: u32, height: u32) -> Self {
        Self::with_clock(root, width, height, ClockPolicy::Virtual)
    }

    /// [`new`](Self::new) on the given clock. Panics when a driver on the
    /// other clock policy is alive on this thread (see the module docs).
    pub fn with_clock(root: Box<dyn Widget>, width: u32, height: u32, clock: ClockPolicy) -> Self {
        agg_gui::ui_thread::mark_current_thread_as_ui_thread();
        let share = ClockShare::join(clock);
        let (width, height) = (width.max(1), height.max(1));
        Self {
            app: App::new(root),
            tracker: LayoutTracker::new(),
            forwarder: InputForwarder::new(),
            painter: Box::new(SoftwarePainter::new(width, height)),
            width,
            height,
            captured: OnceCell::new(),
            background_busy: None,
            clock,
            frames: 0,
            painted: 0,
            close_requested: false,
            _clock: share,
        }
    }

    /// Paint through `painter` instead of the default [`SoftwarePainter`]
    /// (MatterCAD's GPU frame, or a painter with no pixels). It is told the
    /// window's current size before the next frame.
    pub fn with_painter(mut self, mut painter: Box<dyn HeadlessPainter>) -> Self {
        painter.resize(self.width, self.height);
        self.painter = painter;
        self.captured = OnceCell::new();
        self
    }

    /// The painter frames are painted by.
    pub fn painter(&self) -> &dyn HeadlessPainter {
        self.painter.as_ref()
    }

    /// The painter, mutably.
    pub fn painter_mut(&mut self) -> &mut dyn HeadlessPainter {
        self.painter.as_mut()
    }

    /// Tell the driver when the app is waiting on work off the UI thread:
    /// while `busy` reports true the driver is not idle
    /// ([`is_idle`](Self::is_idle)), so the runner's idle waits
    /// ([`AutomationRunner::wait_for_idle`](crate::AutomationRunner::wait_for_idle),
    /// [`wait_for_pending_ui_work`](crate::AutomationRunner::wait_for_pending_ui_work))
    /// keep pumping frames until it is done. Replaces an earlier hook.
    pub fn set_background_busy(&mut self, busy: BackgroundBusy) {
        self.background_busy = Some(busy);
    }

    /// Remove the [`set_background_busy`](Self::set_background_busy) hook.
    pub fn clear_background_busy(&mut self) {
        self.background_busy = None;
    }

    /// Whether the background-busy hook reports work still running (false
    /// without a hook).
    pub fn background_busy(&self) -> bool {
        self.background_busy
            .as_ref()
            .is_some_and(|busy| busy(&self.app))
    }

    /// Whether the UI has settled: no background work running
    /// ([`background_busy`](Self::background_busy)), nothing queued on the
    /// thread's idle queue whose time has come, and no draw requested. A
    /// frame pumped now would do nothing.
    pub fn is_idle(&self) -> bool {
        !self.background_busy()
            && agg_gui::ui_thread::count_expired() == 0
            && !self.app.wants_draw()
    }

    /// The clock this driver runs on.
    pub fn clock_policy(&self) -> ClockPolicy {
        self.clock
    }

    /// The window size in physical pixels.
    pub fn size_px(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn viewport_px(&self) -> Size {
        Size::new(f64::from(self.width), f64::from(self.height))
    }

    /// Resize the window (physical pixels); the next frame lays out again.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.painter.resize(self.width, self.height);
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

    /// Take the pending close request, if any: true once per request. The
    /// run's close phase answers each request it takes by asking the window
    /// whether it may close.
    pub fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
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
        let (width, height) = (self.width, self.height);
        let painter = &mut self.painter;
        let painted = self.tracker.tick(&mut self.app, viewport, mode, |app| {
            painter.paint(app, width, height);
        });
        self.frames += 1;
        if painted {
            self.painted += 1;
            self.captured = OnceCell::new();
        }
        painted
    }

    fn logical_size(&self) -> Size {
        let scale = agg_gui::ux_scale::effective_scale().max(1e-6);
        let px = self.viewport_px();
        Size::new(px.width / scale, px.height / scale)
    }

    /// The painter's last frame: its own framebuffer, or what
    /// [`HeadlessPainter::capture`] reads out (once per painted frame), or a
    /// blank screen of the window's size from a painter with no pixels.
    fn current_screen(&self) -> &Framebuffer {
        if let Some(screen) = self.painter.screen() {
            return screen;
        }
        self.captured.get_or_init(|| {
            self.painter
                .capture()
                .unwrap_or_else(|| Framebuffer::new(self.width, self.height))
        })
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
