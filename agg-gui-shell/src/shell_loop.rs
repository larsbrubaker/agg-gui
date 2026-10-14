//! The event loop body: every winit event the shell reacts to, the reactive
//! control-flow ladder, and device-loss recovery.
//!
//! [`crate::run`] builds a [`ShellLoop`] and hands it to `EventLoop::run`; all
//! the per-event state (coalesced resize, screenshot progress, window bounds)
//! lives here rather than in a stack of captured locals. Input becomes
//! `agg_gui::shell_input::ForwarderEvent`s fed to the loop's
//! [`InputForwarder`], which owns the cursor position, held buttons and
//! modifiers. Each loop iteration and each painted frame drains the thread's
//! `agg_gui::ui_thread` queue. When the loop ends, the device is released
//! within agg-sharp's GPU teardown budget (`finish`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use agg_gui::frame_policy::{wants_frame, FrameDemand};
use agg_gui::shell_input::{ClickCount, ForwarderEvent, InputForwarder};
use agg_gui::{winit_adapter, App};
use agg_gui_wgpu::{Gpu, GpuConfig, RetryWake};
use winit::event::{ElementState, Event, StartCause, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Fullscreen, Window};

use crate::bounds::{
    BoundsAutoSave, ChangeGate, SavedBounds, WindowBoundsStore, WindowedSizeTracker,
};
use crate::config::{RedrawPolicy, ScreenshotConfig};
use crate::host::{ExitAction, ShellControl, ShellHost, WindowGeometry};
use crate::input::{shift_held, touch_event, wheel_delta};
use crate::out_of_view_capture::{OutOfViewMouseCapture, PointerEventKind};
use crate::paint::{schedule_skipped_frame, PaintRequest, Painter};
use crate::position::{SavedPosition, WindowedPositionTracker};
use crate::redraw_schedule::{next_control_flow, paint_blocked, LoopState};
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
    /// Cursor position, held modifiers and buttons, click counting, and the
    /// real-input gate (`agg_gui::shell_input`).
    pub(crate) input: InputForwarder,
    /// Which pointer events reach agg-gui: a drag that leaves the window
    /// keeps delivering, a hover outside it is dropped
    /// (`out_of_view_capture.rs`).
    pub(crate) pointer_capture: OutOfViewMouseCapture,
    /// winit's last reported pointer position (physical pixels), including
    /// moves the filter dropped: what decides whether a press is inside.
    pub(crate) last_pointer: (f64, f64),
    /// The cursor last pushed to the OS, so the platform is only called
    /// when it changes.
    pub(crate) window_cursor: agg_gui::WindowCursor,
    /// Where the forwarder's click policy comes from: the OS setting
    /// (`click_policy::read_os_double_click`), or `None` to keep agg-gui's
    /// default (`ShellConfig::os_click_policy` off).
    pub(crate) click_policy_reader: Option<crate::click_policy::OsDoubleClickReader>,
    /// Paths of the file drag over the window (winit sends one
    /// `HoveredFile` per file); empty when no file drag is in progress.
    pub(crate) hovered_files: Vec<std::path::PathBuf>,
    /// The IME state last sent to the OS (`ime.rs`).
    pub(crate) ime: crate::ime::ImeSync,
    pub(crate) input_since_frame: bool,
    pub(crate) pending_resize: Option<(u32, u32)>,
    /// When a backed-off surface configure retry is due, from the last
    /// paint. Holds the loop in `WaitUntil` instead of `Poll` meanwhile.
    pub(crate) surface_retry_at: Option<Instant>,
    pub(crate) bounds_store: Option<Box<dyn WindowBoundsStore>>,
    pub(crate) bounds_auto: BoundsAutoSave,
    /// Last size seen while the window was neither maximized nor fullscreen —
    /// what gets persisted, so a restore doesn't reopen a windowed window at
    /// the maximized rect.
    pub(crate) windowed_size: WindowedSizeTracker,
    /// Last window position seen while the window was neither maximized,
    /// fullscreen nor minimized (`position.rs`).
    pub(crate) windowed_position: WindowedPositionTracker,
    pub(crate) position_auto: ChangeGate<SavedPosition>,
    pub(crate) maximized: bool,
    /// The monitor the window is on, kept through a measurement of nothing
    /// (off screen, mid hot-plug); refreshed with every geometry report.
    pub(crate) display_size: crate::display_metrics::DisplayUsableSize,
    pub(crate) exit: Rc<Cell<Option<ExitAction>>>,
    pub(crate) error: Rc<RefCell<Option<ShellError>>>,
    pub(crate) finished: bool,
    /// Whether the window gaining or losing key status reaches the app
    /// (agg-sharp's `ForwardPlatformDeactivation`); `ShellSession` turns it
    /// off for an automation run.
    pub(crate) platform_deactivation: bool,
    /// Clears the `agg_gui` host waker when the loop is dropped, whichever way
    /// the loop ended.
    pub(crate) _waker_guard: HostWakerGuard,
}

impl<H: ShellHost> ShellLoop<H> {
    pub(crate) fn handle(&mut self, event: Event<()>, elwt: &ActiveEventLoop) {
        match event {
            Event::WindowEvent { event, .. } => {
                self.window_event(event, elwt);
                // Focus, the caret and the text may all have moved.
                self.ime.sync(&self.window, &self.app);
            }

            // A scheduled `WaitUntil` deadline fired. Belt-and-braces only:
            // the scheduled channel is read non-destructively
            // (`peek_next_draw_deadline`) and a due deadline surfaces through
            // `wants_draw()` on the next `AboutToWait`, so correctness does not
            // depend on catching this event — it only trims latency.
            // The platform finished launching (on macOS, after winit set its
            // default menu): a native menu bar the app installed while it was
            // being built can go up now (`menu_bar::install`).
            Event::NewEvents(StartCause::Init) => crate::menu_bar::finish_launching(),

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

    pub(crate) fn window_event(&mut self, event: WindowEvent, elwt: &ActiveEventLoop) {
        match event {
            WindowEvent::CloseRequested => {
                // The host may veto (unsaved-changes gate) or defer the close
                // — see `ShellHost::on_close_requested`.
                if crate::close_request::close_requested(&mut self.host, &mut self.app) {
                    elwt.exit();
                }
            }

            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                // Coalesced, applied at the top of the next paint: winit can
                // deliver `Resized` from a modal drag-resize loop, and
                // reconfiguring the surface between acquire and present is a
                // validation error.
                self.pending_resize = Some((size.width, size.height));
                self.maximized = crate::maximized::is_maximized(&self.window);
                // winit's `Resized` reports PHYSICAL px. That is the canonical
                // stored unit — it must round-trip through `PhysicalSize` on
                // restore, never `LogicalSize`, or the size ratchets up by the
                // DPI scale factor every launch.
                self.windowed_size.note_resize(
                    (size.width, size.height),
                    self.maximized,
                    self.window.fullscreen().is_some(),
                );
                self.notify_geometry();
                self.window.request_redraw();
            }

            WindowEvent::Moved(position) => {
                self.windowed_position.note_move(
                    SavedPosition {
                        x: position.x,
                        y: position.y,
                    },
                    crate::maximized::is_maximized(&self.window),
                    self.window.fullscreen().is_some(),
                    self.window.is_minimized().unwrap_or(false),
                );
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                agg_gui::set_device_scale(crate::display_metrics::usable_display_scale(
                    scale_factor,
                ));
                // The OS double-click distance is in physical pixels.
                self.refresh_click_policy();
                self.notify_geometry();
                self.window.request_redraw();
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.input_since_frame = true;
                if !self.hovered_files.is_empty() {
                    // A platform that reports the cursor during a file drag
                    // keeps the drop target's feedback following it.
                    let paths = self.hovered_files.clone();
                    self.forward(ForwarderEvent::FileDragHover {
                        x: position.x,
                        y: position.y,
                        paths,
                    });
                }
                self.last_pointer = (position.x, position.y);
                let (kind, button) = self.pointer_capture.move_kind();
                let inside = self.pointer_inside();
                if self.pointer_capture.should_deliver(kind, button, inside) {
                    self.forward(ForwarderEvent::MouseMove {
                        x: position.x,
                        y: position.y,
                        modifiers: None,
                    });
                }
                self.apply_cursor();
            }

            // The window gaining / losing key status (an app switch, a click
            // on another window). Gated like other platform input, so an
            // automation run that turned real input off is not disturbed by
            // the desktop handing focus around mid-run (agg-sharp's
            // `ForwardPlatformDeactivation`).
            WindowEvent::Focused(focused) => {
                if focused {
                    // The user may have changed the OS double-click setting
                    // while another app had focus; the read is a cheap query.
                    self.refresh_click_policy();
                }
                if !self.platform_deactivation {
                    return;
                }
                self.forward(if focused {
                    ForwarderEvent::WindowActivated
                } else {
                    ForwarderEvent::WindowDeactivated
                });
                self.window.request_redraw();
            }

            WindowEvent::CursorLeft { .. } => {
                self.forward(ForwarderEvent::MouseLeave);
            }

            WindowEvent::ModifiersChanged(state) => {
                // Deliver modifier-only changes (Shift mid-drag) to the
                // captured widget, not just to the focused one via KeyDown.
                let mods = winit_adapter::modifiers(state.state());
                self.forward(ForwarderEvent::ModifiersChanged(mods));
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let button = winit_adapter::mouse_button(button);
                self.input_since_frame = true;
                let kind = match state {
                    ElementState::Pressed => PointerEventKind::Down,
                    ElementState::Released => PointerEventKind::Up,
                };
                let inside = self.pointer_inside();
                if !self
                    .pointer_capture
                    .should_deliver(kind, Some(button), inside)
                {
                    return;
                }
                // At the tracked cursor, with the held modifiers.
                self.forward(match state {
                    ElementState::Pressed => ForwarderEvent::MouseDown {
                        at: None,
                        button,
                        modifiers: None,
                        clicks: ClickCount::Auto,
                    },
                    ElementState::Released => ForwarderEvent::MouseUp {
                        at: None,
                        button,
                        modifiers: None,
                    },
                });
                // Press can claim a drag cursor and release re-resolves the
                // hover cursor at the release point; reflect either now
                // rather than waiting for the next move.
                self.apply_cursor();
            }

            WindowEvent::MouseWheel { delta, .. } => {
                self.input_since_frame = true;
                let (delta_x, delta_y) = wheel_delta(
                    delta,
                    shift_held(self.input.modifiers()),
                    self.window.scale_factor() * agg_gui::ux_scale::ux_scale(),
                    cfg!(target_os = "macos"),
                );
                self.forward(ForwarderEvent::Wheel {
                    at: None,
                    delta_x,
                    delta_y,
                    modifiers: None,
                });
            }

            // A trackpad pinch (macOS magnify). agg-sharp's MacTrackpadGestures
            // sends it as a wheel marked FromTrackpadPinch and as two virtual
            // fingers; App::on_trackpad_magnify does both, through the shared
            // magnification conversion. NaN (which winit documents as
            // possible) becomes no zoom there.
            WindowEvent::PinchGesture { delta, phase, .. } => {
                self.input_since_frame = true;
                self.forward(ForwarderEvent::TrackpadPinch {
                    at: None,
                    magnification: delta,
                    phase: winit_adapter::trackpad_phase(phase),
                    modifiers: None,
                });
            }

            // A trackpad rotation (macOS rotate): degrees, counter-clockwise
            // positive, turning the same virtual fingers.
            WindowEvent::RotationGesture { delta, phase, .. } => {
                self.input_since_frame = true;
                self.forward(ForwarderEvent::TrackpadRotate {
                    at: None,
                    degrees: delta as f64,
                    phase: winit_adapter::trackpad_phase(phase),
                });
            }

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                self.input_since_frame = true;
                let Some(key) = winit_adapter::key_event(&key_event, self.input.modifiers()) else {
                    return;
                };
                self.forward(match key_event.state {
                    ElementState::Pressed => ForwarderEvent::KeyDown {
                        key,
                        modifiers: None,
                    },
                    // Key-up matters: chord state, drag modifiers and
                    // held-key repeat all end here. A shell that only
                    // dispatches key-down leaves the app with a stuck key.
                    ElementState::Released => ForwarderEvent::KeyUp {
                        key,
                        modifiers: None,
                    },
                });
            }

            // The input method: composition text and committed text. Plain
            // typing keeps arriving as `KeyboardInput` (winit sends `Commit`
            // only for text an input method composed).
            WindowEvent::Ime(ime) => {
                self.input_since_frame = true;
                if let Some(event) = crate::ime::forwarder_event(ime) {
                    self.forward(event);
                }
                self.window.request_redraw();
            }

            WindowEvent::DroppedFile(path) => {
                self.input_since_frame = true;
                // winit emits one DroppedFile per file in a multi-file drop;
                // forward each separately at the drop position.
                //
                // The tracked cursor is STALE here on Windows: the OS owns the
                // pointer during an OLE drag, winit emits no CursorMoved for
                // it, and its IDropTarget::Drop discards the drop point — so
                // the tracked cursor still says wherever the mouse was before
                // the drag began. Query the live cursor instead; fall back to
                // the tracked position on other platforms.
                let (x, y) = crate::input::live_cursor_in_window(&self.window)
                    .unwrap_or(self.input.cursor());
                // The drop ends the drag: `on_file_dropped` sends
                // `FileDragLeave` first. winit sends no `HoveredFileCancelled`
                // after a drop.
                self.hovered_files.clear();
                self.forward(ForwarderEvent::FileDropped {
                    x,
                    y,
                    paths: vec![path],
                });
            }

            WindowEvent::HoveredFile(path) => {
                self.input_since_frame = true;
                // One event per dragged file, all as the drag enters; each
                // hover carries every path seen so far. The cursor is as stale
                // as for a drop (see above), so query it live.
                self.hovered_files.push(path);
                let (x, y) = crate::input::live_cursor_in_window(&self.window)
                    .unwrap_or(self.input.cursor());
                let paths = self.hovered_files.clone();
                self.forward(ForwarderEvent::FileDragHover { x, y, paths });
                self.window.request_redraw();
            }

            WindowEvent::HoveredFileCancelled => {
                self.input_since_frame = true;
                self.hovered_files.clear();
                self.forward(ForwarderEvent::FileDragLeave);
                self.window.request_redraw();
            }

            WindowEvent::Touch(touch) => {
                self.input_since_frame = true;
                self.forward(touch_event(touch));
                // Touch events arrive outside the mouse arms' redraw
                // signalling — request a frame so gestures render live.
                self.window.request_redraw();
            }

            WindowEvent::RedrawRequested => {
                self.paint(elwt);
            }

            // An occluded window's skipped frame arranges no wake of its own
            // (`RetryWake::OnEvent`, and the paint dropped the immediate draw
            // request — see `paint::schedule_skipped_frame`), so paint it when
            // the window is uncovered rather than at the next unrelated
            // event. macOS is the platform whose surface reports `Occluded`.
            WindowEvent::Occluded(false) => {
                self.window.request_redraw();
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, elwt: &ActiveEventLoop) {
        // Queued UI work first (`agg_gui::ui_thread`), so what it changes is
        // seen by the paint decision below; this also runs it on iterations
        // that paint nothing. Its timed work re-arms the wake through
        // `next_draw_deadline`.
        agg_gui::ui_thread::invoke_pending_actions();
        // Items picked in the native menu bar (`menu_bar`), queued during
        // AppKit's menu tracking and run here on a clean stack.
        if crate::menu_bar::run_pending_activations() {
            self.window.request_redraw();
        }

        // Files the OS asked us to open (Finder, Dock drop). Paths that came
        // in during start-up wait in the queue until this first pass.
        let files = crate::open_files::take_open_files();
        if !files.is_empty() {
            self.host.on_open_files(&mut self.app, files);
            self.window.request_redraw();
        }

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
        } else if wants_frame(&FrameDemand {
            continuous,
            // Input and window events request redraws of their own.
            dirty: false,
            app_wants_draw: self.app.wants_draw(),
            // A deadline wakes the loop through `WaitUntil`, whose
            // `ResumeTimeReached` requests the redraw.
            next_deadline: None,
            now: Instant::now(),
        }) {
            painted = self.paint(elwt);
        }

        {
            let mut exit = self.exit.get();
            let mut control = ShellControl {
                policy: &mut self.policy,
                exit: &mut exit,
                painted,
                pointer_idle: self.input.buttons_down() == 0,
            };
            self.host.on_idle(&mut self.app, &mut control);
            self.exit.set(exit);
        }

        self.save_bounds_if_changed();

        // `wants_draw()` covers due scheduled deadlines, so `Poll` when it (or
        // continuous mode, or a pending capture) is true — unless the surface
        // is backing off after a failed configure, or the window is minimized
        // while its surface is unconfigured,
        // where polling would only spin (see `next_control_flow`). Otherwise
        // re-arm `WaitUntil` from the non-destructive peek — this runs every
        // idle iteration and is idempotent, so an intervening non-repainting
        // event cannot lose the scheduled wake.
        //
        // No `request_redraw` here: `Poll` brings us straight back to
        // `AboutToWait`, which paints. Asking for a redraw as well would paint
        // the same state twice per iteration while anything is animating.
        let state = LoopState {
            capture_pending,
            continuous: self.policy == RedrawPolicy::Continuous,
            app_wants_draw: self.app.wants_draw(),
            paint_blocked: self.paint_blocked(),
            surface_retry_at: self.surface_retry_at,
            app_deadline: self.app.next_draw_deadline(),
            now: Instant::now(),
        };
        elwt.set_control_flow(next_control_flow(&state));

        if self.exit.get().is_some() {
            elwt.exit();
        }
    }

    /// Re-read the OS double-click setting into the forwarder's click policy
    /// (a no-op when the shell keeps agg-gui's default).
    pub(crate) fn refresh_click_policy(&mut self) {
        if let Some(read) = self.click_policy_reader {
            let policy = crate::click_policy::os_click_policy(read, self.window.scale_factor());
            self.input.set_click_policy(policy);
        }
    }

    /// Hand one OS input event to the forwarder, which updates the input
    /// bookkeeping and calls the matching `App` entry point.
    pub(crate) fn forward(&mut self, event: ForwarderEvent) {
        self.input.platform(&mut self.app, event);
    }

    /// Whether winit's last pointer position lies in the content area.
    fn pointer_inside(&self) -> bool {
        let size = self.window.inner_size();
        let (x, y) = self.last_pointer;
        OutOfViewMouseCapture::is_inside(x, y, size.width as f64, size.height as f64)
    }

    /// Push agg-gui's current cursor to the OS when it changed.
    fn apply_cursor(&mut self) {
        if let Some(icon) = self.window_cursor.update(agg_gui::current_cursor_icon()) {
            winit_adapter::apply_cursor(&self.window, icon);
        }
    }

    /// Paint one frame, recovering the device first if it was lost. Returns
    /// whether a frame actually reached the compositor.
    pub(crate) fn paint(&mut self, elwt: &ActiveEventLoop) -> bool {
        // Every painted frame drains `agg_gui::ui_thread` first (a
        // `RedrawRequested` paint does not pass through `about_to_wait`), so
        // what queued work changes shows in this frame.
        agg_gui::ui_thread::invoke_pending_actions();
        // A cursor changed by code (`agg_gui::invalidate_cursor`) reaches
        // the OS even while the mouse rests and painting is blocked.
        self.app.refresh_cursor();
        self.apply_cursor();
        // Minimized with a configure-failure run in progress: every retry
        // against the minimized window would fail and spend the surface
        // retry budget. Restore delivers a nonzero `Resized`, whose arm
        // requests the redraw that resumes painting. A minimized window
        // whose surface is configured paints as usual.
        if self.paint_blocked() {
            schedule_skipped_frame(RetryWake::OnEvent, || {});
            return false;
        }
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
        self.apply_cursor();
        // A painted frame may have moved the caret (layout, scrolling).
        self.ime.sync(&self.window, &self.app);
        match outcome {
            Ok(outcome) => {
                self.surface_retry_at = outcome.retry_at;
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

    /// See [`paint_blocked`]. With no `Gpu` (mid device-loss recovery) the
    /// surface counts as configured: the rebuild is what runs next.
    ///
    /// Runs every frame, so the cheap in-process check goes first and
    /// `Window::is_minimized` is only asked while the surface is
    /// unconfigured: on X11 it can be a server round-trip, which a healthy
    /// app must not pay per frame. A configured surface is never blocked, so
    /// the short-circuit does not change the answer.
    fn paint_blocked(&self) -> bool {
        let configured = self.gpu.as_ref().is_none_or(Gpu::surface_configured);
        !configured && paint_blocked(self.window.is_minimized(), configured)
    }

    /// Rebuild the device and surface after a device loss (TDR, driver reset,
    /// GPU removal, RDP session change).
    ///
    /// wgpu reports the loss out-of-band — nothing in the per-frame API
    /// returns an error — so without this the window paints nothing forever.
    /// A lost device cannot be revived and every resource made from it is dead
    /// with it, so the whole bundle is rebuilt and the host is told to drop
    /// its own cached GPU resources.
    ///
    /// The C# port does the same thing in
    /// `PlatformWin32/win32/WebGpuControl.cs::TryRecoverDevice` (agg-sharp), down
    /// to the closing repaint request; it additionally guards against re-entering
    /// recovery from a failure raised by recovery itself, which the `&mut self`
    /// borrow here rules out for free.
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
            scale_factor: crate::display_metrics::usable_display_scale(self.window.scale_factor()),
            display_size: self.display_size.get(),
        }
    }

    fn notify_geometry(&mut self) {
        if let Some(monitor) = self.window.current_monitor() {
            let size = monitor.size();
            self.display_size
                .set(f64::from(size.width), f64::from(size.height));
        }
        let geometry = self.geometry();
        self.host.on_geometry_changed(geometry);
    }

    fn current_bounds(&self) -> SavedBounds {
        let (width, height) = self.windowed_size.size();
        SavedBounds {
            width,
            height,
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
        let idle = self.input.buttons_down() == 0;
        if self.bounds_auto.should_save(idle, bounds) {
            store.save(bounds);
        }
        if let Some(position) = self.windowed_position.position() {
            if self.position_auto.should_save(idle, position) {
                store.save_position(position);
            }
        }
    }

    /// Final flush, once, however the loop ended.
    pub(crate) fn finish(&mut self) {
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
            if let Some(position) = self.windowed_position.position() {
                store.save_position(position);
            }
        }
        self.host.on_exit(&mut self.app);
        // The close never waits on the GPU for long (agg-sharp `GpuTeardown`,
        // 5 s): the drain runs on its own thread, and only a drain that came
        // back in time is followed by the release, here, while the window
        // still exists. A device still busy past the budget is leaked instead
        // of holding the close (see `Gpu::release_within_budget`).
        if let Some(gpu) = self.gpu.take() {
            gpu.release_within_budget(agg_gui_wgpu::GPU_TEARDOWN_BUDGET);
        }
    }
}
