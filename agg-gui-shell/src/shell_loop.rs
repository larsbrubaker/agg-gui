//! The event loop body: every winit event the shell reacts to, the reactive
//! control-flow ladder, and device-loss recovery.
//!
//! [`crate::run`] builds a [`ShellLoop`] and hands it to `EventLoop::run`; all
//! the per-event state (cursor position, modifiers, held buttons, coalesced
//! resize, screenshot progress, window bounds) lives here rather than in a
//! stack of captured locals.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use agg_gui::{winit_adapter, App, Modifiers};
use agg_gui_wgpu::{Gpu, GpuConfig};
use winit::event::{ElementState, Event, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Fullscreen, Window};

use crate::bounds::{BoundsAutoSave, SavedBounds, WindowBoundsStore};
use crate::config::{RedrawPolicy, ScreenshotConfig};
use crate::host::{ExitAction, ShellControl, ShellHost, WindowGeometry};
use crate::input::{dispatch_touch, shift_held, wheel_delta};
use crate::paint::{PaintRequest, Painter};
use crate::screenshot::capture_exhausted;
use crate::waker::HostWakerGuard;
use crate::ShellError;

/// Owns everything the running shell touches. One instance is moved into the
/// winit callback.
pub(crate) struct ShellLoop<H: ShellHost> {
    pub(crate) window: Arc<Window>,
    /// `None` only during device-loss recovery, where the dead device and its
    /// surface are dropped before the replacement is created — two live
    /// surfaces on one window is not portable. Every user early-returns
    /// instead of unwrapping.
    pub(crate) gpu: Option<Gpu>,
    pub(crate) gpu_config: GpuConfig,
    pub(crate) painter: Painter,
    pub(crate) app: App,
    pub(crate) host: H,
    pub(crate) policy: RedrawPolicy,
    pub(crate) screenshot: Option<ScreenshotConfig>,
    pub(crate) screenshot_done: bool,
    /// When a pending capture last saw a painted frame — the wall-clock budget
    /// that stops a surface which never becomes presentable from spinning
    /// forever.
    pub(crate) screenshot_last_paint: Instant,
    pub(crate) cursor: (f64, f64),
    pub(crate) mods: Modifiers,
    pub(crate) mouse_buttons_down: u32,
    pub(crate) input_since_frame: bool,
    pub(crate) pending_resize: Option<(u32, u32)>,
    pub(crate) bounds_store: Option<Box<dyn WindowBoundsStore>>,
    pub(crate) bounds_auto: BoundsAutoSave,
    /// Last size seen while the window was NOT maximized — what gets
    /// persisted, so a restore doesn't reopen a windowed window at the
    /// maximized rect.
    pub(crate) last_windowed: (u32, u32),
    pub(crate) maximized: bool,
    pub(crate) exit: Rc<Cell<Option<ExitAction>>>,
    pub(crate) error: Rc<RefCell<Option<ShellError>>>,
    pub(crate) finished: bool,
    /// Clears the `agg_gui` host waker when the loop is dropped, whichever way
    /// the loop ended.
    pub(crate) _waker_guard: HostWakerGuard,
}

impl<H: ShellHost> ShellLoop<H> {
    pub(crate) fn handle(&mut self, event: Event<()>, elwt: &ActiveEventLoop) {
        match event {
            Event::WindowEvent { event, .. } => self.window_event(event, elwt),

            // A scheduled `WaitUntil` deadline fired. Belt-and-braces only:
            // the scheduled channel is read non-destructively
            // (`peek_next_draw_deadline`) and a due deadline surfaces through
            // `wants_draw()` on the next `AboutToWait`, so correctness does not
            // depend on catching this event — it only trims latency.
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                self.window.request_redraw();
            }

            // A background thread signalled agg-gui, which woke us through the
            // installed host waker. Bump the invalidation epoch so the next
            // paint re-runs layout: work that landed off-thread (a decoded
            // image, a fetched document, a new video frame) is usually pulled
            // in from `layout()`, which the layout-skip key would otherwise
            // skip. Note `request_draw` does NOT re-enter the waker, so this
            // cannot loop.
            Event::UserEvent(()) => {
                agg_gui::animation::request_draw();
            }

            Event::AboutToWait => self.about_to_wait(elwt),

            Event::LoopExiting => self.finish(),

            _ => {}
        }
    }

    fn window_event(&mut self, event: WindowEvent, elwt: &ActiveEventLoop) {
        match event {
            WindowEvent::CloseRequested => {
                elwt.exit();
            }

            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                // Coalesced, applied at the top of the next paint: winit can
                // deliver `Resized` from a modal drag-resize loop, and
                // reconfiguring the surface between acquire and present is a
                // validation error.
                self.pending_resize = Some((size.width, size.height));
                self.maximized = self.window.is_maximized();
                if !self.maximized {
                    // winit's `Resized` reports PHYSICAL px. That is the
                    // canonical stored unit — it must round-trip through
                    // `PhysicalSize` on restore, never `LogicalSize`, or the
                    // size ratchets up by the DPI scale factor every launch.
                    self.last_windowed = (size.width, size.height);
                }
                self.notify_geometry();
                self.window.request_redraw();
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                agg_gui::set_device_scale(scale_factor);
                self.notify_geometry();
                self.window.request_redraw();
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                self.input_since_frame = true;
                self.app.on_mouse_move(self.cursor.0, self.cursor.1);
                winit_adapter::apply_cursor(&self.window, agg_gui::current_cursor_icon());
            }

            WindowEvent::CursorLeft { .. } => {
                self.app.on_mouse_leave();
            }

            WindowEvent::ModifiersChanged(state) => {
                self.mods = winit_adapter::modifiers(state.state());
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let btn = winit_adapter::mouse_button(button);
                self.input_since_frame = true;
                let (x, y) = self.cursor;
                match state {
                    ElementState::Pressed => {
                        self.mouse_buttons_down = self.mouse_buttons_down.saturating_add(1);
                        self.app.on_mouse_down(x, y, btn, self.mods);
                    }
                    ElementState::Released => {
                        self.mouse_buttons_down = self.mouse_buttons_down.saturating_sub(1);
                        self.app.on_mouse_up(x, y, btn, self.mods);
                    }
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                self.input_since_frame = true;
                let (dx, dy) = wheel_delta(delta, shift_held(self.mods));
                let (x, y) = self.cursor;
                self.app.on_mouse_wheel_xy_mods(x, y, dx, dy, self.mods);
            }

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                self.input_since_frame = true;
                let Some(key) = winit_adapter::key_event(&key_event, self.mods) else {
                    return;
                };
                match key_event.state {
                    ElementState::Pressed => self.app.on_key_down(key, self.mods),
                    // Key-up matters: chord state, drag modifiers and
                    // held-key repeat all end here. A shell that only
                    // dispatches key-down leaves the app with a stuck key.
                    ElementState::Released => self.app.on_key_up(key, self.mods),
                }
            }

            WindowEvent::Touch(touch) => {
                self.input_since_frame = true;
                dispatch_touch(&mut self.app, touch);
                // Touch events arrive outside the mouse arms' redraw
                // signalling — request a frame so gestures render live.
                self.window.request_redraw();
            }

            WindowEvent::RedrawRequested => {
                self.paint(elwt);
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, elwt: &ActiveEventLoop) {
        // App-requested fullscreen toggles (`agg_gui::fullscreen`).
        if agg_gui::fullscreen::take_request() {
            let now_fullscreen = self.window.fullscreen().is_none();
            self.window
                .set_fullscreen(now_fullscreen.then_some(Fullscreen::Borderless(None)));
            agg_gui::fullscreen::set_active(now_fullscreen);
            self.notify_geometry();
            self.window.request_redraw();
        }

        let capture_pending = self.screenshot.is_some() && !self.screenshot_done;
        let continuous = self.policy == RedrawPolicy::Continuous;
        let mut painted = false;
        if capture_pending {
            // A pending deterministic capture needs the frame count to advance
            // on its own; `ControlFlow::Wait` would stall forever with no user
            // input.
            painted = self.paint(elwt);
            if !self.screenshot_done && capture_exhausted(self.screenshot_last_paint.elapsed()) {
                self.fail(
                    ShellError::Screenshot(format!(
                        "gave up after {:.0}s without a presentable surface",
                        self.screenshot_last_paint.elapsed().as_secs_f64()
                    )),
                    elwt,
                );
            }
        } else if continuous || self.app.wants_draw() {
            painted = self.paint(elwt);
        }

        {
            let mut exit = self.exit.get();
            let mut control = ShellControl {
                policy: &mut self.policy,
                exit: &mut exit,
                painted,
                pointer_idle: self.mouse_buttons_down == 0,
            };
            self.host.on_idle(&mut self.app, &mut control);
            self.exit.set(exit);
        }

        self.save_bounds_if_changed();

        // `wants_draw()` covers due scheduled deadlines, so `Poll` when it (or
        // continuous mode, or a pending capture) is true. Otherwise re-arm
        // `WaitUntil` from the non-destructive peek — this runs every idle
        // iteration and is idempotent, so an intervening non-repainting event
        // cannot lose the scheduled wake.
        //
        // No `request_redraw` here: `Poll` brings us straight back to
        // `AboutToWait`, which paints. Asking for a redraw as well would paint
        // the same state twice per iteration while anything is animating.
        let poll = capture_pending || self.policy == RedrawPolicy::Continuous;
        if poll || self.app.wants_draw() {
            elwt.set_control_flow(ControlFlow::Poll);
        } else if let Some(t) = self.app.next_draw_deadline() {
            elwt.set_control_flow(ControlFlow::WaitUntil(t));
        } else {
            elwt.set_control_flow(ControlFlow::Wait);
        }

        if self.exit.get().is_some() {
            elwt.exit();
        }
    }

    /// Paint one frame, recovering the device first if it was lost. Returns
    /// whether a frame actually reached the compositor.
    pub(crate) fn paint(&mut self, elwt: &ActiveEventLoop) -> bool {
        if let Err(e) = self.recover_lost_device() {
            self.fail(e, elwt);
            return false;
        }
        let Some(gpu) = self.gpu.as_mut() else {
            return false;
        };
        let capture = self
            .screenshot
            .as_ref()
            .filter(|_| !self.screenshot_done)
            .cloned();
        let outcome = self.painter.paint(
            gpu,
            &self.window,
            &mut self.app,
            &mut self.host,
            PaintRequest {
                pending_resize: &mut self.pending_resize,
                input_since_last_frame: self.input_since_frame,
                capture: capture.as_ref(),
            },
        );
        match outcome {
            Ok(outcome) => {
                if outcome.painted {
                    self.input_since_frame = false;
                    self.screenshot_last_paint = Instant::now();
                }
                if outcome.captured {
                    self.screenshot_done = true;
                    elwt.exit();
                }
                outcome.painted
            }
            Err(e) => {
                self.fail(e, elwt);
                false
            }
        }
    }

    /// Rebuild the device and surface after a device loss (TDR, driver reset,
    /// GPU removal, RDP session change).
    ///
    /// wgpu reports the loss out-of-band — nothing in the per-frame API
    /// returns an error — so without this the window paints nothing forever.
    /// A lost device cannot be revived and every resource made from it is dead
    /// with it, so the whole bundle is rebuilt and the host is told to drop
    /// its own cached GPU resources.
    fn recover_lost_device(&mut self) -> Result<(), ShellError> {
        if !self.gpu.as_ref().is_some_and(|g| g.device_lost()) {
            return Ok(());
        }
        log::warn!("agg-gui-shell: wgpu device lost — rebuilding device and surface");
        // Drop the dead device and its surface BEFORE creating the
        // replacement: two live surfaces for one window is not portable.
        drop(self.gpu.take());
        let size = self.window.inner_size();
        let gpu = Gpu::new(
            Arc::clone(&self.window),
            (size.width.max(1), size.height.max(1)),
            self.gpu_config,
        )
        .map_err(ShellError::Gpu)?;
        self.painter.rebuild(&gpu);
        self.host.on_gpu_rebuilt(&mut self.app, &gpu);
        self.gpu = Some(gpu);
        // The recovered surface starts blank; the app's next frame is a full
        // repaint, not an incremental one.
        agg_gui::animation::request_draw();
        Ok(())
    }

    /// Record the first error and end the loop. Later errors are dropped: the
    /// first one is the cause, and the shell is already shutting down.
    fn fail(&mut self, error: ShellError, elwt: &ActiveEventLoop) {
        if let Ok(mut slot) = self.error.try_borrow_mut() {
            if slot.is_none() {
                *slot = Some(error);
            }
        }
        elwt.exit();
    }

    fn geometry(&self) -> WindowGeometry {
        let size = self.window.inner_size();
        WindowGeometry {
            width: size.width,
            height: size.height,
            maximized: self.maximized,
            fullscreen: self.window.fullscreen().is_some(),
            scale_factor: self.window.scale_factor(),
        }
    }

    fn notify_geometry(&mut self) {
        let geometry = self.geometry();
        self.host.on_geometry_changed(geometry);
    }

    fn current_bounds(&self) -> SavedBounds {
        SavedBounds {
            width: self.last_windowed.0,
            height: self.last_windowed.1,
            maximized: self.maximized,
        }
    }

    /// Persist the window bounds when they changed and no mouse button is
    /// held, so a drag-resize doesn't hit the store on every intermediate
    /// size.
    fn save_bounds_if_changed(&mut self) {
        let Some(store) = self.bounds_store.as_ref() else {
            return;
        };
        let bounds = self.current_bounds();
        if self
            .bounds_auto
            .should_save(self.mouse_buttons_down == 0, bounds)
        {
            store.save(bounds);
        }
    }

    /// Final flush, once, however the loop ended.
    fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        if let Some(store) = self.bounds_store.as_ref() {
            // Unconditional, and BEFORE `on_exit`: the last change may have
            // happened with a button still held (closing from a drag), which
            // the idle gate skipped — and an app whose store only records the
            // bounds writes them out from its own `on_exit` flush.
            store.save(self.current_bounds());
        }
        self.host.on_exit(&mut self.app);
    }
}

/// Everything the shell knows once the window and GPU exist but before the app
/// is built — what a builder closure needs to size its first layout or share
/// the device with an app-owned renderer.
pub struct ShellInit<'a> {
    pub(crate) window: &'a Arc<Window>,
    pub(crate) gpu: &'a Gpu,
    pub(crate) restored_bounds: Option<SavedBounds>,
}

impl ShellInit<'_> {
    /// The OS window. Handy for a title the app computes, or for platform
    /// integration the shell doesn't wrap.
    pub fn window(&self) -> &Arc<Window> {
        self.window
    }

    /// The live device + surface bundle. An app with its own wgpu renderer
    /// builds it from this device.
    pub fn gpu(&self) -> &Gpu {
        self.gpu
    }

    /// Surface size in physical pixels — already clamped to the GPU limit.
    pub fn size(&self) -> (u32, u32) {
        (self.gpu.config().width, self.gpu.config().height)
    }

    /// What the [`WindowBoundsStore`] returned, if anything. An app that
    /// keeps other window state alongside the bounds can use this to tell a
    /// restored launch from a first one.
    pub fn restored_bounds(&self) -> Option<SavedBounds> {
        self.restored_bounds
    }

    /// `agg_gui::device_scale()`, already applied from the window.
    pub fn device_scale(&self) -> f64 {
        agg_gui::device_scale()
    }
}
