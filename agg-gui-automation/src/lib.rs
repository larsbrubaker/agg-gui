//! GUI automation for agg-gui — the port of agg-sharp's `GuiAutomation`
//! assembly (`AutomationRunner`, `TypedKeyParser`, `SearchRegion`, ...).
//!
//! Tests drive a real widget tree through the same input entry points the
//! shells use, find widgets by name, and wait on conditions.  This crate is
//! test-only: consumers take it as a dev-dependency so watchdogs, stack dumps
//! and image matching stay out of product builds, and so its live mode can
//! depend on `agg-gui-shell` (winit and wgpu), which core agg-gui must not.
//!
//! # How a test runs
//!
//! - **Simulated input is real input.** The runner never calls a widget's
//!   `on_event`. Every move, press, key and wheel turn becomes a
//!   `ForwarderEvent` fed through `agg_gui::shell_input::InputForwarder`, the
//!   bookkeeping the native and web shells feed OS events through
//!   ([`input`]). Hover, capture, focus, modals, tooltips and the on-screen
//!   keyboard therefore behave as they do for a user, and a test cannot pass
//!   by going around them. Input is dispatched before the frame's idle drain,
//!   the order the shells use.
//! - **The test drives the pump.** `App`, widgets and agg-gui's thread-locals
//!   cannot cross threads, so the test body runs on the UI thread and every
//!   runner call is synchronous: it sends its input, then pumps frames until
//!   its condition holds. When a call returns, the UI has taken the event.
//!   What owns the `App` and runs its frames is a [`driver`]:
//!   [`HeadlessDriver`] (no window or GPU, deterministic) or, with the `live`
//!   feature, `LiveDriver` (a real window).
//! - **One thread per test.** [`show_window_and_execute_tests`] runs each
//!   test on a fresh thread. Fresh thread-locals (focus, modifiers, tooltip,
//!   animation, idle queue, clock) isolate it, so tests run in parallel with
//!   nothing to reset, and the calling thread stays free to time out a stuck
//!   body ([`execute`]). An application's own harness, which owns its thread
//!   and driver, wraps them with [`AutomationRunner::attach`].
//! - **Two clocks.** Behaviour runs on UI time (`agg_gui::clock`): delays,
//!   waits and their timeouts, and pointer pacing. Headless, that is a
//!   virtual clock advanced one frame interval per pumped frame, so a test
//!   never sleeps ([`ClockPolicy::Real`] is for an application whose worker
//!   threads run in real time). The hang watchdogs (test budget, bring-up,
//!   close) run on the wall clock, because they must fire when UI time stops.
//!
//! # Coordinates and options
//!
//! - The runner's pointer position is physical pixels, Y-down from the
//!   window's top-left corner: what `App`'s entry points take.
//! - A [`Point2D`] offset onto a widget is logical units, Y-up from the
//!   widget's lower-left corner (or from its centre with
//!   `ClickOrigin::Center`); a window point given to
//!   `set_mouse_cursor_position` is Y-up logical.
//! - Optional arguments are option structs with `Default` ([`ClickOpts`],
//!   [`WaitOpts`], [`DragOpts`], [`TypeOpts`]).
//! - Pacing and matching settings live in each runner's [`AutomationConfig`]
//!   rather than in process-wide state, so parallel tests cannot disturb each
//!   other.
//!
//! # What a test author must know
//!
//! - **A worker that serves a UI thread other than the main one** wakes it
//!   through that thread's queue: capture `ui_thread::current_queue()` on the
//!   UI thread, then call `UiQueue::run_on_idle` or
//!   `UiQueue::signal_async_state_change` from the worker. An unbound
//!   worker's `animation::signal_async_state_change` wakes only the main
//!   queue's owner, so a headless test on its own thread would never see it.
//! - **Process-wide agg-gui state is shared by parallel tests.** The
//!   platform has a per-thread override
//!   (`agg_gui::platform::override_platform_for_thread`); the input profile,
//!   tilt and fullscreen requests have none, so a test that changes them
//!   must not run beside one that reads them.
//! - **Headless software paint can be slow on a large tree.** Frames paint
//!   only when the app wants to draw, so idle `delay` and `wait_for` frames
//!   stay cheap.
//!
//! # Features
//!
//! - `stack-dump` — `thread_stack_dump`: all-thread stack capture for hang
//!   reports, over rust-minidump (pure Rust).
//! - `live` — `LiveDriver` and the live tests, separate `harness = false`
//!   binaries (`tests/live/`): winit wants its event loop on the main thread
//!   on macOS and allows one per process. agg-gui's CI does not run them,
//!   because its test machines have no display. Run them on a desktop with
//!   `cargo test -p agg-gui-automation --features live --test
//!   automation_runner_live_tests`, or on Linux under `xvfb-run` with a
//!   Vulkan driver (lavapipe through `ShellConfig::with_force_fallback_adapter`).
//!
//! There is no browser driver: every runner call blocks while it pumps
//! frames, and a wasm page cannot block. [`UiDriver`] is the seam an async
//! driver would implement.
//!
//! # Modules
//!
//! - [`keys`] — C#'s `Keys` key codes (what typed strings parse into).
//! - `key_mapping` — `Keys` strokes to agg-gui `Key`/`Modifiers`.
//! - [`typed_key_parser`] — `TypedKeyParser`: the `^`, `^+`, `{Token}` spelling.
//! - [`search_region`] — `ScreenRectangle` / `SearchRegion`.
//! - [`waits`] — `StaticDelay`, the wall-clock condition wait.
//! - [`driver`] — `HeadlessDriver` (an `App` on a pluggable painter, a
//!   software framebuffer by default, and a virtual clock) and `HeadlessWindow` (C#'s `SystemWindow` for runner-less
//!   tests), and with the `live` feature `LiveDriver` (a real
//!   `agg-gui-shell` window the test pumps).
//! - [`overlay`] — `RenderMouse`: the simulated pointer a live window draws
//!   on its presented frames.
//! - [`probe`] — `ProbeWidget`, the stand-in for a plain C# `GuiWidget`.
//! - [`execute`] — `show_window_and_execute_tests`: one run on its own UI
//!   thread, with the bring-up and test budgets, the close protocol (the
//!   `close_window` callback, veto, close budget, forced close), `AutomationWindow`, and `AutomationError`.
//! - [`runner`] — `AutomationRunner`, what a test body drives: its
//!   configuration (`AutomationConfig` with C#'s defaults), its waits
//!   (`delay`, `wait_for`, `assert`, `wait_for_draw`, `wait_for_idle`) and its name lookups
//!   (`get_widget_by_name`, `wait_for_name`, `wait_for_widget_enabled`, ...)
//!   and its pointer gestures (`click_by_name`, `double_click_by_name`, `right_click_by_name`,
//!   `move_to_by_name`, `set_mouse_cursor_position`, ...) and drags
//!   (`drag_by_name`, `drop_by_name`, `drag_drop_by_name`, `drag_widget`,
//!   `drag_to_position`, `drop`), file drags and drops (`drop_files_by_name`,
//!   `drop_file_data_by_name`, `file_drag_hover_by_name`, `file_drag_leave`),
//!   the mouse wheel (`scroll_wheel`, `scroll_wheel_by_name`) and keyboard (`type_text`,
//!   `press_modifier_keys`/`release_modifier_keys` with `ModifierKeys`,
//!   `select_all`, `select_none`) and image searches (`get_current_screen`,
//!   `image_exists`, `wait_for_image`) and image gestures (`click_image`,
//!   `drag_image`, `drop_image`, `drag_drop_image`) and `complete_dialog`.
//! - [`image_match`] — `FindLeastSquaresMatch`: one image inside another.
//! - [`image_source`] — `ImageNeedle` and `ImageSource`: needles by file name
//!   (C#'s images folder, or an application's own loader).
//! - [`input`] — `InputMethod` (C# `IInputMethod`) and `SimulatedInput`
//!   (`AggInputMethods`): pointer and keyboard input through the shells'
//!   input forwarder.
//! - [`paint_geometry`] — painting's transform and clip chain, replayed
//!   without a draw context (pixel snapping, children and slot clips, the
//!   overlay pass).
//! - [`name_pick`] — which same-named thing a click by name goes to and
//!   where on it the click lands, as pure functions of a tree.
//! - [`pointer_reach`] — `PointerReach`: whether a press can get to a widget.
//! - [`pointer_state`] — C#'s `UnderMouseState`, `MouseCaptured`,
//!   `ChildHasMouseCaptured` and `Focused` for a found widget.
//! - [`dialog`] — `AutomationFileDialog` (C# `AutomationDialogProvider`):
//!   the in-canvas file dialog an application shows under automation, and
//!   the `ModalPresenter` it is shown through.
//! - [`startup_failure_log`] — `StartupFailureLog`: an application's startup
//!   failure, appended to `widget_not_found_message`.
//! - [`tree_query`] — `WidgetHandle`, `NamedHit`, lookup by name, screen and
//!   clipped rectangles, `ActuallyVisibleOnScreen`, inherited enabled state,
//!   `Parents<T>`/`Children<T>`.
//! - `thread_stack_dump` (feature `stack-dump`) — `ThreadStackDump`: the
//!   all-thread stack capture, a minidump of this process written and walked
//!   into a text report, with thread labels and the capture retry.

pub mod dialog;
pub mod driver;
pub mod execute;
pub mod image_match;
pub mod image_source;
pub mod input;
mod key_mapping;
pub mod keys;
pub mod name_pick;
pub mod overlay;
pub mod paint_geometry;
pub mod pointer_reach;
pub mod pointer_state;
pub mod probe;
pub mod runner;
pub mod search_region;
pub mod startup_failure_log;
#[cfg(feature = "stack-dump")]
pub mod thread_stack_dump;
pub mod tree_query;
pub mod typed_key_parser;
pub mod waits;

pub use agg_gui::NamedTarget;
pub use dialog::{
    parse_dialog_text, AutomationFileDialog, AutomationModalPresenter, FileDialogKind,
    FileDialogResult, ModalPresenter, AUTOMATION_DIALOG_TEXT_EDIT, AUTOMATION_DIALOG_WARNING,
};
pub use driver::{
    BackgroundBusy, ClockPolicy, FrameKind, HeadlessDriver, HeadlessPainter, HeadlessWindow,
    SoftwarePainter, UiDriver,
};
#[cfg(feature = "live")]
pub use driver::{LiveDriver, LiveHost};
pub use execute::{show_window_and_execute_tests, AutomationError, AutomationWindow, RunOptions};
pub use image_match::{find_least_squares_match, LeastSquaresMatch};
pub use image_source::{decode_needle, DirectoryImageSource, ImageNeedle, ImageSource};
pub use input::{InputMethod, MouseAction, Point2D, SimulatedInput};
pub use keys::Keys;
pub use name_pick::HitPlacement;
pub use probe::{ProbeLog, ProbeWidget};
pub use runner::{
    AutomationConfig, AutomationRunner, ClickOpts, ClickOrigin, DragDropOpts, DragOpts,
    ImageDragDropOpts, ImageOpts, ModifierKeys, TypeOpts, WaitOpts,
};
pub use search_region::{ScreenRectangle, SearchRegion};
pub use startup_failure_log::StartupFailureLog;
pub use tree_query::{NamedHit, WidgetHandle};
pub use typed_key_parser::{ParseError, TypedKey, TypedKeyParser};
pub use waits::static_delay;
