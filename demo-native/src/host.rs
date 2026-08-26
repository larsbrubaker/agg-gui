//! The demo's [`ShellHost`] — everything `agg-gui-shell` does not own.
//!
//! `agg-gui-shell` runs the window, the event loop and the wgpu present; this
//! file is what is genuinely demo-specific and gets threaded back in through
//! the shell's hooks:
//!
//! - the live inspector's edit queues and tree snapshot (`render_app_frame`),
//!   which is why the frame body is overridden rather than defaulted,
//! - the GPU-direct screenshot capture / save / copy, which has to run between
//!   `end_frame` and `present`,
//! - saved-state persistence (window bounds via [`StateFileBounds`], the rest
//!   through `agg_gui::persistence::AutoSave` on the idle tick),
//! - the run-mode → redraw-policy mirror, the runaway-repaint detector, the
//!   Ctrl+Shift+D draw report, and the Render tab's Relaunch button.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui::{App, DrawCtx};
use agg_gui_shell::{
    Frame, SavedBounds, ShellControl, ShellHost, WgpuGfxCtx, WindowBoundsStore, WindowGeometry,
};
use demo_wgpu::render_app_frame;

use crate::state_file::{load_saved_state, save_state_to_disk, serialize_state};

/// Window bounds inside the demo's own saved-state file.
///
/// `save` only records the bounds; the file itself is written by the demo's
/// `AutoSave` tick (and by `on_exit`), which serializes window size together
/// with the rest of the session state. Splitting the write would mean two
/// writers racing for one file.
pub struct StateFileBounds {
    pub(crate) latest: Rc<Cell<Option<SavedBounds>>>,
}

impl WindowBoundsStore for StateFileBounds {
    fn load(&self) -> Option<SavedBounds> {
        let state = load_saved_state()?;
        match (state.window_w, state.window_h) {
            (Some(width), Some(height)) => Some(SavedBounds {
                width,
                height,
                maximized: state.window_maximized,
            }),
            _ => None,
        }
    }

    fn save(&self, bounds: SavedBounds) {
        self.latest.set(Some(bounds));
    }
}

/// Per-frame and per-idle plumbing for the native demo.
pub struct DemoHost {
    pub(crate) handles_show_inspector: Rc<Cell<bool>>,
    pub(crate) inspector_nodes: Rc<RefCell<Vec<agg_gui::InspectorNode>>>,
    pub(crate) hovered_bounds: Rc<RefCell<Option<agg_gui::InspectorOverlay>>>,
    pub(crate) base_edits: Rc<RefCell<Vec<agg_gui::WidgetBaseEdit>>>,
    #[cfg(feature = "reflect")]
    pub(crate) inspector_edits: Rc<RefCell<Vec<agg_gui::InspectorEdit>>>,
    pub(crate) run_mode: Rc<Cell<demo_ui::RunMode>>,
    pub(crate) screen_size: Rc<Cell<(u32, u32)>>,
    pub(crate) frame_history: Rc<RefCell<demo_ui::FrameHistory>>,
    pub(crate) window_maximized: Rc<Cell<bool>>,
    pub(crate) window_fullscreen: Rc<Cell<bool>>,
    pub(crate) screenshot_request: Rc<Cell<bool>>,
    pub(crate) screenshot_available: Rc<Cell<bool>>,
    pub(crate) screenshot_save_pending: Rc<Cell<bool>>,
    pub(crate) screenshot_copy_pending: Rc<Cell<bool>>,
    pub(crate) screenshot_capture_seq: Rc<Cell<u64>>,
    pub(crate) debug_report_requested: Rc<Cell<bool>>,
    pub(crate) relaunch_requested: Rc<Cell<bool>>,
    pub(crate) state: demo_ui::StateAccessor,
    /// Last bounds the shell handed to [`StateFileBounds`] — what the state
    /// file records, so a maximized session still restores to its windowed
    /// size.
    pub(crate) bounds: Rc<Cell<Option<SavedBounds>>>,
    pub(crate) auto_save: agg_gui::persistence::AutoSave,
    pub(crate) runaway: demo_ui::RunawayDetector,
    /// Held for the process lifetime: the runtime owns the detached signaling
    /// tasks behind the Screen Share demo.
    pub(crate) _screen_share: crate::screen_share::ScreenShare,
}

impl DemoHost {
    fn last_windowed(&self) -> Option<(u32, u32)> {
        self.bounds.get().map(|b| (b.width, b.height))
    }

    fn serialize(&self) -> String {
        serialize_state(&self.state, self.last_windowed())
    }
}

impl ShellHost for DemoHost {
    fn on_frame(&mut self, app: &mut App, frame: &Frame) {
        self.frame_history
            .borrow_mut()
            .push(frame.duration.as_secs_f32() * 1000.0);

        // A real runaway manifests as reactive frames rendered every idle
        // iteration with no intervening input. Fire the report ONCE (the
        // detector latches) so it is captured even if the user doesn't notice
        // the spinning.
        let reactive = self.run_mode.get() == demo_ui::RunMode::Reactive;
        if self
            .runaway
            .note_frame(reactive, frame.input_since_last_frame)
        {
            crate::emit_draw_report(app, "AUTO-DETECTED RUNAWAY");
        }
    }

    fn paint(&mut self, app: &mut App, ctx: &mut WgpuGfxCtx, frame: &Frame) {
        // Not `agg_gui_shell::default_paint`: the demo's live inspector has to
        // drain its edit queues and refresh its tree snapshot around layout and
        // paint, which is what `render_app_frame` does.
        render_app_frame(
            ctx,
            app,
            frame.width,
            frame.height,
            frame.duration.as_secs_f64() * 1000.0,
            self.handles_show_inspector.get(),
            &self.inspector_nodes,
            &self.hovered_bounds,
            &self.base_edits,
            #[cfg(feature = "reflect")]
            &self.inspector_edits,
        );
    }

    fn after_paint(&mut self, ctx: &mut WgpuGfxCtx, _frame: &Frame) {
        // GPU-direct screenshot flow: a single render per frame. When a
        // capture is requested we issue ONE extra `copy_texture_to_texture`
        // after `end_frame()` and before `present()` — pure GPU work, no
        // readback. The screenshot widget's preview pane samples the capture
        // texture directly each frame via `DrawCtx::draw_captured_screenshot`.
        //
        // wgpu's swap chain takes ownership of the surface texture at
        // `present`, which is why this is a hook and not app code.
        if self.screenshot_request.get() && ctx.capture_screenshot() {
            self.screenshot_request.set(false);
            self.screenshot_available.set(true);
            // Bump the capture seq + wake the loop so `ImageView`'s
            // `needs_draw` flips true exactly once and the new screenshot
            // displays on the very next frame, instead of waiting for an
            // unrelated event (mouse hover, etc.) to dirty the screenshot
            // window's backbuffer cache.
            self.screenshot_capture_seq
                .set(self.screenshot_capture_seq.get().wrapping_add(1));
            agg_gui::animation::request_draw();
        }

        // Drain deferred Save / Copy actions — the click handlers can't read
        // pixels themselves (no `DrawCtx` access in event dispatch), so they
        // flip a pending flag and the readback runs here, after
        // `capture_screenshot` has populated the capture texture.
        if self.screenshot_save_pending.replace(false) {
            let (rgba, w, h) = ctx.read_captured_screenshot();
            if !rgba.is_empty() {
                if let Err(err) =
                    agg_gui::screenshot::download_rgba_as_png(&rgba, w, h, "agg-gui-screenshot.png")
                {
                    eprintln!("screenshot save failed: {err}");
                }
            }
        }
        if self.screenshot_copy_pending.replace(false) {
            let (rgba, w, h) = ctx.read_captured_screenshot();
            if !rgba.is_empty() {
                if let Err(err) = agg_gui::screenshot::copy_rgba_to_clipboard(&rgba, w, h) {
                    eprintln!("screenshot copy failed: {err}");
                }
            }
        }
    }

    fn on_geometry_changed(&mut self, geometry: WindowGeometry) {
        self.screen_size.set((geometry.width, geometry.height));
        self.window_maximized.set(geometry.maximized);
        self.window_fullscreen.set(geometry.fullscreen);
    }

    fn on_idle(&mut self, app: &mut App, control: &mut ShellControl<'_>) {
        // Manual draw-report capture: Ctrl+Shift+D latched this via the demo-ui
        // global key handler; build + emit it here, where the `App` is in hand
        // (the handler runs inside App dispatch and can't).
        if self.debug_report_requested.replace(false) {
            crate::emit_draw_report(app, "manual Ctrl+Shift+D");
        }

        // Continuous run mode keeps the app repainting unconditionally (used by
        // the perf graphs). Continuous SCREENSHOT capture is driven from inside
        // `ImageView::paint` in the screenshot demo — re-arming it here would
        // un-scope it from "window is open", so closing the screenshot window
        // with the checkbox still on would leave the loop spinning forever.
        control.set_redraw_policy(match self.run_mode.get() {
            demo_ui::RunMode::Continuous => agg_gui_shell::RedrawPolicy::Continuous,
            _ => agg_gui_shell::RedrawPolicy::Reactive,
        });

        if !control.painted() {
            // Loop went idle — reset the detector so a later runaway is caught
            // fresh.
            self.runaway.note_idle();
        }

        // Diff serialized state against the last-saved blob and write only on
        // change, gated on no button held so a drag/resize doesn't hammer the
        // disk.
        let DemoHost {
            auto_save,
            state,
            bounds,
            ..
        } = self;
        let last_windowed = bounds.get().map(|b| (b.width, b.height));
        auto_save.tick(
            control.pointer_idle(),
            || serialize_state(state, last_windowed),
            save_state_to_disk,
        );

        // Render-tab Relaunch button. The shell flushes state (`on_exit`) and
        // spawns the replacement only after the loop has exited, so the child
        // reads the state this run just wrote — including the new MSAA sample
        // count, which only applies at surface configuration.
        if self.relaunch_requested.replace(false) {
            control.request_relaunch();
        }
    }

    fn on_exit(&mut self, _app: &mut App) {
        save_state_to_disk(&self.serialize());
    }
}
