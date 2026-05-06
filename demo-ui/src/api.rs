use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::InspectorNode;
use agg_gui::InspectorOverlay;

use crate::backend_panel::{FrameHistory, RunMode};
use crate::state::StateAccessor;

// ── Platform hook ─────────────────────────────────────────────────────────────

/// Which host shell is running the demo.  Consumed by the System window's
/// Render tab so platform-specific controls (MSAA as a five-value segmented
/// selector on native vs. a boolean on the web, "Relaunch" vs "Refresh"
/// button label) stay inside demo-ui — demo-native and demo-wasm only
/// declare which variant they are and hand in the hook closure that
/// actually performs the platform-specific restart.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlatformKind {
    Native,
    Web,
}

/// Platform-specific hooks that demo-ui calls from the Render tab.
#[derive(Clone)]
pub struct PlatformHooks {
    pub kind: PlatformKind,
    /// MSAA sample count actually in effect on the currently-running GL
    /// surface.  Native hosts pass `gl_config.num_samples()`; web hosts
    /// pass `4` when `antialias: true` was honoured at canvas creation,
    /// `0` otherwise.  The Render tab compares this to the pending
    /// `msaa_samples` cell so the Relaunch / Refresh button only
    /// activates when the user has actually changed something.
    pub running_msaa: u8,
    /// Invoked when the user clicks the Render tab's Relaunch / Refresh
    /// button.  Expected behaviour:
    ///   - **Native**: flush any pending save, spawn a fresh copy of the
    ///     process, exit the current one so the new GL surface picks up
    ///     the saved MSAA request.
    ///   - **Web**: call `window.location.reload()` so the browser
    ///     re-creates the canvas with the saved `antialias` flag.
    pub on_reload: Rc<dyn Fn()>,
    /// Invoked when a UI action selects a font that has not been loaded yet.
    /// Platform shells own the actual bytes: native can read from disk, while
    /// WASM can fetch the asset URL asynchronously and install it later.
    pub on_font_request: Rc<dyn Fn(&str, &str)>,
}

impl PlatformHooks {
    pub fn native(running_msaa: u8, on_reload: impl Fn() + 'static) -> Self {
        Self {
            kind: PlatformKind::Native,
            running_msaa,
            on_reload: Rc::new(on_reload),
            on_font_request: Rc::new(|_, _| {}),
        }
    }
    pub fn web(running_msaa: u8, on_reload: impl Fn() + 'static) -> Self {
        Self {
            kind: PlatformKind::Web,
            running_msaa,
            on_reload: Rc::new(on_reload),
            on_font_request: Rc::new(|_, _| {}),
        }
    }

    pub fn with_font_requester(mut self, on_font_request: impl Fn(&str, &str) + 'static) -> Self {
        self.on_font_request = Rc::new(on_font_request);
        self
    }
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Handles returned by `build_demo_ui` — shared cells used by the platform harness.
pub struct DemoHandles {
    pub show_inspector: Rc<Cell<bool>>,
    pub inspector_nodes: Rc<RefCell<Vec<InspectorNode>>>,
    pub hovered_bounds: Rc<RefCell<Option<InspectorOverlay>>>,
    /// Pending WidgetBase live-edits (margin, anchors).  The platform harness
    /// drains and applies via `agg_gui::apply_widget_base_edit` each frame.
    /// Always present — does not require the `reflect` feature.
    pub base_edits: Rc<RefCell<Vec<agg_gui::WidgetBaseEdit>>>,
    /// Pending inspector edits — the platform harness drains and applies via
    /// `agg_gui::apply_inspector_edit` each frame after layout.  Only present
    /// with the `reflect` cargo feature.
    #[cfg(feature = "reflect")]
    pub inspector_edits: Rc<RefCell<Vec<agg_gui::InspectorEdit>>>,
    pub cube_visible: Rc<Cell<bool>>,
    /// Backend panel run mode. Platform hosts read this to choose between
    /// reactive idle waits and continuous redraw.
    pub run_mode: Rc<Cell<RunMode>>,
    pub screen_size: Rc<Cell<(u32, u32)>>,
    pub frame_history: Rc<RefCell<FrameHistory>>,
    /// Fullscreen state of the OS window.  The platform harness sets this
    /// cell whenever the window transitions.
    pub window_fullscreen: Rc<Cell<bool>>,
    /// Maximized (not fullscreen) state of the OS window.
    pub window_maximized: Rc<Cell<bool>>,
    /// When set to `true`, the platform harness captures the frame buffer on
    /// the NEXT fully-rendered frame, writes the RGBA8 data + dimensions into
    /// `screenshot_image`, then resets this flag.  Set to `true` from any
    /// widget (e.g. the Screenshot demo button) to request a capture.
    pub screenshot_request: Rc<Cell<bool>>,
    /// Latest captured frame.  `None` until at least one capture completes.
    /// Top-down RGBA8; first `width * 4` bytes are the TOP row.
    ///
    /// Wrapped in `Arc<Vec<u8>>` so the GL texture cache can key on the
    /// Arc's pointer identity (via `draw_image_rgba_arc`).  Using a bare
    /// `Vec<u8>` triggered false cache hits — the allocator reused
    /// addresses across consecutive captures and the content-hash key
    /// (first/last 8 bytes) collided on screenshots whose corners were
    /// stable, causing stale frames to be bound.
    pub screenshot_image: Rc<RefCell<Option<(Arc<Vec<u8>>, u32, u32)>>>,
    /// Transient flag set by the harness during the FIRST render pass of a
    /// capture frame.  Read by the screenshot demo's preview pane so it
    /// paints an empty frame (not the stale previous capture) — this keeps
    /// the captured pixels free of the screenshot-of-a-screenshot recursion.
    pub screenshot_capturing: Rc<Cell<bool>>,
    /// Set to `true` once a capture has succeeded at least once this session
    /// (either via GPU-direct `DrawCtx::capture_screenshot` on wgpu, or via
    /// the legacy `screenshot_image` path on software backends).  Drives the
    /// Save / Copy buttons' enabled state — independent of which path the
    /// platform uses, so the buttons light up in both flows.
    pub screenshot_available: Rc<Cell<bool>>,
    /// Click-deferred Save: the Save button sets this from the event-dispatch
    /// closure (no `ctx` available there); the platform harness drains it in
    /// its post-paint pass and performs the GPU readback + PNG encode + disk
    /// write (or download trigger on WASM).
    pub screenshot_save_pending: Rc<Cell<bool>>,
    /// Click-deferred Copy.  Same flow as `screenshot_save_pending`, but the
    /// harness pipes the bytes to the system clipboard.
    pub screenshot_copy_pending: Rc<Cell<bool>>,
    /// Continuous-capture flag mirrored from the screenshot demo's "Capture
    /// continuously" checkbox.  Read by the screenshot demo's `ImageView`
    /// from inside `paint` to re-arm `screenshot_request` each frame —
    /// keeping the continuous loop scoped to "screenshot window is open".
    pub screenshot_continuous: Rc<Cell<bool>>,
    /// Monotonic counter the platform harness increments after a successful
    /// `DrawCtx::capture_screenshot`.  The screenshot demo's `ImageView`
    /// compares this against its locally-cached value in `needs_draw` so
    /// the parent Window's retained backbuffer invalidates exactly once
    /// per capture — the post-click "the new screenshot doesn't appear
    /// until I move the mouse" bug.  Bumping a counter (instead of
    /// calling `signal_async_state_change`) keeps continuous-capture
    /// performance bounded: only the screenshot Window invalidates,
    /// not every retained Window in the app.
    pub screenshot_capture_seq: Rc<Cell<u64>>,
    pub state: StateAccessor,
}
