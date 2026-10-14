//! `LiveDriver` (feature `live`): the runner's driver over a real
//! `agg-gui-shell` window, pumped through [`ShellSession`] on the UI thread
//! the test body runs on (design: `docs/design/gui-automation.md`, section 7).
//!
//! - **Input.** Simulated input goes through [`ShellSession::send_simulated`],
//!   into the same `InputForwarder` the shell's winit arms feed. Real input
//!   and window activation are switched off for the driver's life (agg-sharp
//!   `RealInputIgnored`, `DesktopDeactivationIgnored`): the person at the
//!   machine moving the mouse, or the desktop moving windows under a still
//!   cursor, cannot move the test's pointer, end its hovers or type into it.
//! - **Frames.** One pump is one pass of the shell's loop with no wait; the
//!   window paints when the app asks to, or when the frame is forced. The UI
//!   runs on the real clock ([`ClockPolicy::Real`]), so the runner paces each
//!   frame to the frame interval.
//! - **Pixels.** Every painted frame is read back ([`LiveHost`]'s
//!   `after_paint`) for `current_screen`, before the simulated mouse is drawn
//!   over it (`overlay::render_mouse`, the shell's presented-only overlay
//!   pass): the watcher sees the pointer, image searches never do. The
//!   read-back costs a GPU round trip per painted frame, which a test window
//!   can afford.
//!
//! winit allows one event loop per process, created on the main thread on
//! macOS, so a process opens one live driver, from `main`: live tests are
//! `harness = false` binaries (`tests/live/`).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use agg_gui::shell_input::ForwarderEvent;
use agg_gui::{App, Framebuffer, Size, Widget};
use agg_gui_shell::{CopySrc, Frame, ShellConfig, ShellError, ShellHost, ShellSession, WgpuGfxCtx};

use super::{ClockPolicy, FrameKind, UiDriver};
use crate::overlay::{render_mouse, SimulatedMouse};

/// What the driver shares with its host: the overlay's pointer and switch,
/// and the last read-back frame.
#[derive(Default)]
struct LiveState {
    draw_simulated_mouse: bool,
    mouse: SimulatedMouse,
    screen: Option<Framebuffer>,
}

/// The [`ShellHost`] of a live driver's window: reads each painted frame
/// back and draws the simulated mouse over the presented frame.
pub struct LiveHost {
    state: Rc<RefCell<LiveState>>,
}

impl ShellHost for LiveHost {
    fn after_paint(&mut self, ctx: &mut WgpuGfxCtx, _frame: &Frame) {
        let (rgba, width, height) = ctx.read_screenshot();
        if !rgba.is_empty() {
            self.state.borrow_mut().screen = Some(framebuffer_from_top_down(&rgba, width, height));
        }
    }

    fn wants_presented_overlay(&self) -> bool {
        self.state.borrow().draw_simulated_mouse
    }

    fn paint_presented_overlay(&mut self, _app: &App, ctx: &mut WgpuGfxCtx, frame: &Frame) {
        let mouse = self.state.borrow().mouse;
        render_mouse(ctx, &mouse, f64::from(frame.height), frame.device_scale);
    }
}

/// A read-back (top row first) as a [`Framebuffer`] (bottom row first).
fn framebuffer_from_top_down(rgba: &[u8], width: u32, height: u32) -> Framebuffer {
    let mut screen = Framebuffer::new(width, height);
    let row = width as usize * 4;
    for (y, dst) in screen.pixels_mut().chunks_exact_mut(row).enumerate() {
        let src = (height as usize - 1 - y) * row;
        dst.copy_from_slice(&rgba[src..src + row]);
    }
    screen
}

/// A real window driven by the runner; see the module docs.
pub struct LiveDriver {
    session: ShellSession<LiveHost>,
    state: Rc<RefCell<LiveState>>,
    screen: Framebuffer,
    close_requested: bool,
}

impl LiveDriver {
    /// Open a window for `root` with `config` (frame read-back is required,
    /// whatever `config` says), with real input and window activation
    /// switched off and the simulated mouse drawn (C#'s
    /// `DrawSimulatedMouse` default).
    pub fn open(config: ShellConfig, root: Box<dyn Widget>) -> Result<Self, ShellError> {
        let state = Rc::new(RefCell::new(LiveState {
            draw_simulated_mouse: true,
            ..LiveState::default()
        }));
        let host = LiveHost {
            state: Rc::clone(&state),
        };
        let config = config.with_copy_src(CopySrc::Required);
        let mut session = ShellSession::open(config, move |_init| Ok((App::new(root), host)))?;
        session.set_platform_input_enabled(false);
        session.set_platform_deactivation_enabled(false);
        let size = session.window().inner_size();
        let screen = state
            .borrow_mut()
            .screen
            .take()
            .unwrap_or_else(|| Framebuffer::new(size.width.max(1), size.height.max(1)));
        Ok(Self {
            session,
            state,
            screen,
            close_requested: false,
        })
    }

    /// The window's session (its real-event queue, gates, close).
    pub fn session(&self) -> &ShellSession<LiveHost> {
        &self.session
    }

    /// The window's session, mutably.
    pub fn session_mut(&mut self) -> &mut ShellSession<LiveHost> {
        &mut self.session
    }

    /// C# `DrawSimulatedMouse`: draw the simulated pointer over presented
    /// frames (on by default).
    pub fn set_draw_simulated_mouse(&mut self, draw: bool) {
        self.state.borrow_mut().draw_simulated_mouse = draw;
        agg_gui::animation::request_draw();
    }

    /// Whether the simulated pointer is drawn.
    pub fn draw_simulated_mouse(&self) -> bool {
        self.state.borrow().draw_simulated_mouse
    }

    /// Whether something asked the window to close
    /// ([`UiDriver::request_close`]).
    pub fn close_requested(&self) -> bool {
        self.close_requested
    }

    /// Take the pending close request, if any: true once per request.
    pub fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// Hand the overlay the pointer as the forwarder now tracks it, and ask
    /// for a frame so the drawn pointer follows it.
    fn sync_mouse(&mut self) {
        let mouse = SimulatedMouse::from_forwarder(self.session.forwarder());
        let mut state = self.state.borrow_mut();
        if state.mouse != mouse {
            state.mouse = mouse;
            if state.draw_simulated_mouse {
                agg_gui::animation::request_draw();
            }
        }
    }
}

impl UiDriver for LiveDriver {
    fn app(&self) -> &App {
        self.session.app()
    }

    fn app_mut(&mut self) -> &mut App {
        self.session.app_mut()
    }

    fn pump(&mut self, kind: FrameKind) -> bool {
        if kind == FrameKind::Forced {
            agg_gui::animation::request_draw();
        }
        self.sync_mouse();
        let before = self.session.presented_frames();
        self.session.pump(Some(Duration::ZERO));
        if let Some(screen) = self.state.borrow_mut().screen.take() {
            self.screen = screen;
        }
        self.session.presented_frames() > before
    }

    fn logical_size(&self) -> Size {
        let scale = agg_gui::ux_scale::effective_scale().max(1e-6);
        let (width, height) = self.size_px();
        Size::new(f64::from(width) / scale, f64::from(height) / scale)
    }

    /// The last painted frame as it was read back, without the simulated
    /// mouse.
    fn current_screen(&self) -> &Framebuffer {
        &self.screen
    }

    fn size_px(&self) -> (u32, u32) {
        let size = self.session.window().inner_size();
        (size.width.max(1), size.height.max(1))
    }

    fn send(&mut self, event: ForwarderEvent) {
        self.session.send_simulated(event);
        self.sync_mouse();
    }

    fn request_close(&mut self) {
        self.close_requested = true;
    }

    fn clock_policy(&self) -> ClockPolicy {
        ClockPolicy::Real
    }

    fn background_busy(&self) -> bool {
        false
    }

    fn is_idle(&self) -> bool {
        agg_gui::ui_thread::count_expired() == 0 && !self.session.app().wants_draw()
    }
}
