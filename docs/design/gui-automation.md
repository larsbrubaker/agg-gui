# GUI Automation for agg-gui: porting agg-sharp's GuiAutomation (design)

*Design for the port of agg-sharp's GuiAutomation (MatterCAD's `Submodules/agg-sharp/GuiAutomation`). Open work only: delete sections as slices land.*

## 0. Summary

- **New crate:** `agg-gui-automation` in the agg-gui workspace, the counterpart of C#'s separate `GuiAutomation` assembly.
- **The simulated mouse is real input.** Mouse and keyboard go through the same `App::on_*` entry points the native and web shells call. The mouse moves in `MouseMoveSteps` (5) eased steps (Cubic.Out). Presses and releases follow C#'s exact event shapes, including explicit click counts.
- **Threading:** C# runs the test on one thread and the UI on another, joined by `RunOnIdle` and reset events. Rust instead runs the test body on the UI thread, and every runner call pumps frames itself.
  - **Headless:** `HeadlessDriver` owns the `App`, a painter (a software framebuffer by default) and a virtual clock. It is deterministic and needs no window or GPU.
  - **Live window:** `LiveDriver` pumps a real `agg-gui-shell` window with winit's `pump_app_events`.
- **Two clocks:**
  - Virtual time: `Delay`, `WaitFor`, timeouts and pointer pacing.
  - Wall time: the hang watchdogs (test budget, bring-up, close).
- **One thread per test:** `show_window_and_execute_tests` runs each test on a fresh thread. That isolates agg-gui's thread-local state and lets the caller time out a stuck body, which is what C#'s `Task.WhenAny` does.
- **Order of work:** the remaining slices fill agg-gui's gaps test-first, port the 18 remaining Agg Automation Tests 1:1, then finish moving `mattercad-app-test` onto the runner (section 9).

## 1. What exists today

**C# (`agg-sharp/GuiAutomation`)**
- `AutomationRunner.cs` (2336 lines) holds the API plus `ShowWindowAndExecuteTests`, with its load, test and close watchdogs.
- `AggInputMethods` sends mouse events through `UiThread.RunOnIdle`.
- `TypedKeyParser` handles the `^`, `^+` and `{Token}` spelling.
- Smaller pieces:
  - `SearchRegion`/`ScreenRectangle`: Y-down ints.
  - `PointerReach`: prefers widgets whose ancestors accept the press.
  - `AutomationDialogProvider`: a SystemWindow with an "Automation Dialog TextEdit" field.
  - `RealInputIgnored` / `DesktopDeactivationIgnored`.
  - `StartupFailureLog`.
  - `ThreadStackDump`: createdump plus ClrMD.

**Tests:** 13 files, 12 classes (`AutomationRunnerTests` is a partial class split across 2 files), 103 tests. Six of the 13 files never call the runner and drive widgets directly: TextEdit, Mac bindings, ThreadStackDump, most of MouseInteraction, ToolTip and FlowLayout. Their ports run in a `HeadlessWindow` (or, for FlowLayout, agg-gui's own `tests/flow_layout_agg_sharp`).

**agg-gui**
- `App` (`agg-gui/src/widget/app.rs` with `app/pointer.rs` and `app/keyboard.rs`) takes Y-down physical input:
  - `on_mouse_move`, `on_mouse_down`/`on_mouse_up(x, y, button, mods)`
  - `on_key_down`/`on_key_up(Key, mods)`, where `Key::Char` is the character (there is no separate KeyPress)
  - wheel, file drop and modifier calls.
- Tree lookup: `Widget::id()` (overridden by only a few agg-gui widgets), `find_widget_by_id`, `find_widget_screen_rect`.
- `path_anchor` tracks widget identity by heap address; `agg_gui::WidgetAnchor` exposes it (a path that follows its widget through reorders and reports it gone), with `widget::walk_path` and `App::focused_path`.
- Nearly all state is thread-local. Exceptions: the process-wide platform (which a thread can override with `platform::override_platform_for_thread`) and the input profile are process-global atomics.
- Text editing follows C#'s Mac and Windows key bindings (`platform::use_mac_key_bindings`, `widgets::text_key_bindings`): Option word-wise and Command line- and document-wise on Mac, Control word-wise elsewhere, in both `TextField` and `TextArea`.
- Widget names (`WidgetBase.name`, `Widget::with_name`, default `id()`, `widgets::Named`), typed downcasts (`Widget::as_any`/`as_any_mut` on the core widgets) and an origin-placing container (`widgets::AbsoluteLayout` over `WidgetBase.origin`) exist.
- `agg_gui::ui_thread` is the idle queue (`run_on_idle`, `run_on_idle_after`, intervals, `invoke_pending_actions`), one queue per UI thread, delays on `agg_gui::clock`, drained by both shells every loop iteration. Frame-loop wakeups are counted per queue: a post or `signal_async_state_change` wakes the thread that drains that queue (an unbound worker's, the main queue's owner), never another UI thread. Queued work runs under panic containment and reports through `agg_gui::report_unhandled` (`agg_gui::unhandled`, a per-thread handler set with `set_unhandled_handler`; with none, the first panic is re-raised after the rest of the drain).
- `agg_gui::frame_policy` holds the frame policy both shells use: `LayoutKey` (size, device scale, invalidation epoch), `LayoutTracker` (needs-layout, `layout_if_needed`, and the GPU-free `tick` a headless driver runs), and `wants_frame` (the paint decision). `agg_gui::shell_input::InputForwarder` holds the input bookkeeping both shells feed their OS events through as `ForwarderEvent`s.
- `Widget::is_enabled` reports a widget's own enabled state (default true; `Button`, `SegmentedControl` and `ChevronWidget` report theirs).
- Click counts: `App::on_mouse_down_clicks` takes a stated count, the forwarder passes every press's count, and widgets read `event::current_click_count()` / `event::is_double_click()` (C#'s `Clicks` / `IsDoubleClick`); `MultiClickTracker` honours only a stated count.
- Under the mouse: the hovered chain and the widgets it covers (under a sibling drawn above them, `UnderMouseNotFirst`) send bounds `MouseEnter`/`MouseLeave`, the chain's deepest widget gets first-under-mouse `MouseOver`/`MouseOut` (C#'s `MouseEnter`/`MouseLeave`), a press updates them as a move does, capture follows the captured widget's own area and freezes everything else, a release over the capture holder sends no hover-refresh move, and `App::hovered_chain`/`first_under_mouse`/`under_mouse_state`/`captured_path` plus the thread snapshot `agg_gui::under_mouse_state_of(WidgetId)` answer C#'s `UnderMouseState`, `MouseCaptured` and `ChildHasMouseCaptured`.
- `agg_gui::observe_events(WidgetId, callback)` watches the events one widget receives (C#'s per-widget `MouseMove +=` and friends) without wrapping it.
- `Widget::scroll_rect_into_view` (C#'s `ScrollableWidget.ScrollIntoView`, minimum scroll) is implemented by `ScrollView` and `TextArea`. `TextArea` carries C#'s `InternalTextEditWidget` caret API (`with_line_spacing`, `content_height`, `insert_bar_position`, the clamping `set_char_index_to_insert_before`/`set_selection_index_to_start_before`, `copy_selection`).
- The central tooltip controller (`widgets::tooltip::controller`) is C#'s `ToolTipManager`: `current_text`, `clear`, `observe_tooltips` (`TooltipEvent::Shown`/`Popped`), and C#'s reshow rule (`timings::hover_delay`).
- `widgets::RadioButton` is C#'s standalone `RadioButton`: a widget per option, exclusive with the radio buttons in its parent (settled by the dispatch walk before its callbacks run), individually disableable, with `on_checked_state_changed`/`on_click` and `check_radio_child` for a programmatic check.

**`mattercad-app-test`**
- `TestHarness` owns a `HeadlessDriver` (app window builder, the app's wgpu painter, background-busy hook) with an attached `AutomationRunner` (`runner()`/`runner_mut()`); `run_test`/`new_part_tab_test`, `runner_automation` (MatterCADTests' extension methods) and `AutomationFileService` (the dialog answered by `complete_dialog` through `AutomationFileDialog` on the popup layer) exist. Name lookups are `name_pick::find_on_screen`, and the app's View3D, menus and popup hosts report named targets through `find_named_targets`.
- Still on the old helpers: `automation.rs`'s `click_by_name`/`wait_for`/`wait_for_name`/`scroll_into_view` (teleporting clicks, a frame-counted `MAX_WAIT_FRAMES = 120`), `harness_input.rs`'s `click`/`drag`, and the ad hoc `type_keys` (4 `{}` tokens and a leading `^`). Test files that still call them are listed under M5-M9 in section 9.

## 2. Where the code goes

**Crate `agg-gui/agg-gui-automation`** (workspace member; consumers use it as a dev-dependency). It exists with `keys`, `key_mapping`, `typed_key_parser`, `search_region`, `waits` (`static_delay`), `driver` (`HeadlessDriver`, `HeadlessPainter`, `SoftwarePainter`, `BackgroundBusy`, `HeadlessWindow`, `ClockPolicy`, `FrameKind`, `UiDriver`), `probe` (`ProbeWidget`) and `tree_query` (`WidgetHandle`, `find_by_name`, `find_named_hits` (widgets and the named targets widgets report), `local_rect_to_screen`, `placement`/`screen_rect`/`clipped_rect`, `actually_visible_on_screen`, `parents`/`children` and their `_of_type` forms), `execute` (`show_window_and_execute_tests`, `RunOptions`, `AutomationWindow`, `AutomationError`, the close phase in `execute/close.rs`), `image_match` (`find_least_squares_match`, `LeastSquaresMatch`), `pointer_reach` (`can_reach`, `prefer_reachable`), `pointer_state` (`under_mouse_state`, `mouse_captured`, `child_has_mouse_captured`, `focused` for a handle), `input` (`InputMethod`, `SimulatedInput`, `Point2D`, `MouseAction`) and `runner` (`AutomationRunner` with `AutomationConfig`, `attach`/`into_driver`, `mark_test_complete`, driver access, the waits in `runner/waits.rs`, the name lookups in `runner/named.rs`, `scroll_into_view` in `runner/scroll.rs`, the pointer gestures in `runner/pointer.rs`, the drags in `runner/drag.rs`, the keyboard (`type_text`, `ModifierKeys`, `press_modifier_keys`/`release_modifier_keys`, `select_all`/`select_none`) in `runner/keyboard.rs` the image searches (`get_current_screen`, `get_screen_region`, `capture_region`, `image_exists`, `wait_for_image`) in `runner/images.rs` and the image gestures (`click_image`, `drag_image`, `drop_image`, `drag_drop_image`, `set_image_source`) in `runner/image_gestures.rs`), `image_source` (`ImageNeedle`, `ImageSource`, `DirectoryImageSource`, `decode_needle`); the other modules and members below are still to come.
- It keeps test-only code (watchdogs, stack dumps, image matching) out of product builds.
- Its optional live mode depends on `agg-gui-shell` (winit and wgpu), which core `agg-gui` must not.
- Features, each added with the slice whose code uses it (none exist yet; headless is the default):
  - `live = ["dep:agg-gui-shell"]` (slice 31)
  - `stack-dump = ["dep:minidump-writer", "dep:minidump-processor", "dep:minidump-unwind"]` (slice 30)

Modules (each under 800 lines, each opening with a purpose comment):

```
src/lib.rs                 crate docs, re-exports
src/runner/mod.rs          (exists) AutomationRunner, AutomationConfig, MarkTestComplete
src/runner/named.rs        (exists) Get*/Wait*/NameExists/NamedWidgetExists/ChildExists/GetRegionByName/GetObjectByName, named targets included
src/runner/scroll.rs       (exists) ScrollIntoView
src/runner/pointer.rs      (exists) stepped moves, ClickOrigin/ClickOpts, Click*/DoubleClickByName/RightClick*/MoveToByName/SetMouseCursorPosition
src/runner/drag.rs         (exists) DragOpts/DragDropOpts, Drag*/Drop*
src/runner/keyboard.rs     (exists) Type, ModifierKeys, Press/ReleaseModifierKeys, SelectAll/None
src/runner/waits.rs        (exists) Delay, WaitFor/WaitUntil, Assert, WaitForPendingUiWork, WaitforDraw
src/input.rs               (exists) InputMethod trait (IInputMethod) + SimulatedInput (AggInputMethods), MouseConsts→MouseAction, keyboard members; current_screen to come
src/paint_geometry.rs      (exists) painting's transform and clip chain, replayed for the lookups
src/name_pick.rs           (exists) GetWidgetByName's pick among same-named hits and where the click lands; find_on_screen
src/pointer_reach.rs       (exists) PointerReach
src/tree_query.rs          (exists) WidgetHandle, NamedHit (GetByNameResults, with its `target`), find_named_hits, screen/clip rects, ActuallyVisibleOnScreen, inherited Enabled, Parents/Children
src/image_match.rs         (exists) FindLeastSquaresMatch over agg_gui::Framebuffer
src/driver/mod.rs          (exists) UiDriver trait, FrameKind, ClockPolicy
src/driver/headless.rs     (exists) HeadlessDriver
src/driver/painter.rs      (exists) HeadlessPainter, SoftwarePainter
src/driver/thread_clock.rs (exists) the clock shared by a thread's drivers
src/driver/window.rs       (exists) HeadlessWindow (the "SystemWindow" for tests that use no runner)
src/driver/live.rs         (feature live) LiveDriver over agg_gui_shell::ShellSession
src/probe.rs               (exists) ProbeWidget: generic C#-GuiWidget stand-in (name, bounds, colour, event log/callbacks, C# Click semantics)
src/overlay.rs             RenderMouse (simulated pointer drawing), drawn by the LiveDriver (slice 31)
src/dialog/mod.rs          (exists) AutomationFileDialog (AutomationDialogProvider), ModalPresenter
src/dialog/overlay.rs      (exists) AutomationWindow's built-in overlay layer, AutomationModalPresenter
src/runner/dialog.rs       (exists) complete_dialog (MatterCAD's CompleteDialog)
src/startup_failure_log.rs (exists) StartupFailureLog
src/thread_stack_dump/     (feature stack-dump) ThreadStackDump
tests/*.rs                 the 103 ports (section 8)
tests/live/*.rs            live tests, `harness = false` (winit on macOS needs the main thread)
```

## 3. Public API, member by member

**Conventions**
- Builder-style options: C# optional parameters become `&ClickOpts`, `&WaitOpts` and `&DragOpts` with `Default`.
- `Point2D` offsets keep C#'s meaning: Y-up, measured from the widget's lower-left corner; `ClickOrigin::Center` adds the center hint.
- Window points (`SetMouseCursorPosition(window, x, y)`) are Y-up logical, as in C#.
- The runner's own pointer position is Y-down physical pixels, which is what the `App` takes.
- Process-wide C# statics become per-runner `AutomationConfig` fields with the same defaults, so parallel tests cannot interfere with each other.

**Configuration and control**

| C# | Rust |
|---|---|
| `MatchLimit` (50) | `config.match_limit` |
| `RequireTestCompletion` / `TestWasCompleted` / `MarkTestComplete()` | `config.require_test_completion`, `test_was_completed()`, `mark_test_complete()` |
| static `TimeToMoveMouse` (0.1), `MouseMoveSteps` (5), `UpDelaySeconds` (0.1) | `config.time_to_move_mouse`, `config.mouse_move_steps`, `config.up_delay`. Same values, Cubic.Out easing and step formula. |
| `DrawSimulatedMouse` | `RunOptions.draw_simulated_mouse` (`InputType`/`OverrideInputSystem`/static `InputMethod` are `AutomationRunner::set_input_method`, default `SimulatedInput`) |
| `InterpolationType` (unused) | Dropped as dead code; noted in the port comment. |
| `GetCurrentScreen()` | `get_current_screen() -> Framebuffer` (exists): headless, a copy of the painter's last frame (the software framebuffer by default, a plugged-in painter's `capture`, or a blank screen of the window's size from a painter with no pixels); live, a read-back. |
| `RenderMouse` | `overlay::render_mouse` (slice 31). C# draws it on the window's `AfterDraw` while `DrawSimulatedMouse` (default true) is set, as a watcher's aid: a radius-10 circle stroked black (3) and white (2), filled green while the left button is down with "S"/"C" for held Shift/Control plus the click count above 1. The LiveDriver draws it after `App::paint` on the presented frame only, never into `get_current_screen` captures, so image searches never see it; headless runs have no watcher and draw nothing. |
| `Dispose`, `KeyDown`/`KeyUp` (throw NotImplemented in C#) | `Drop`. The two key methods are left out. |

**Name lookup** (the rest of it exists in `runner/named.rs`)

| C# | Rust |
|---|---|
| `GetByNameResults.NamedObject` | `NamedHit.target: Option<NamedTarget>` (exists); `None` when the widget carries the name |
| `GetObjectByName` | `get_object_by_name(name, &WaitOpts) -> Option<NamedHit>` (exists): the first hit, a widget or a `NamedTarget` |
| `SetTarget` (`DebugShowBounds`) | agg-gui has no bounds overlay; `get_widget_by_name` flashes nothing until one exists |
| `WidgetNotFoundMessage` | `widget_not_found_message(op, name)` (exists), with `StartupFailureLog::append_to` applied |

**Images**

| C# | Rust |
|---|---|
| `ImageExists`, `WaitForImage` (needle as an image) | exist: `image_exists`/`wait_for_image(&Framebuffer, secs, Option<&SearchRegion>)`, least-squares match in `image_match.rs` under `match_limit` |

**Run entry point**

| C# | Rust |
|---|---|
| `ShowWindowAndExecuteTests(window, test, secs = 30, images, closeWindow, timeoutIsTheExpectedOutcome)` | `show_window_and_execute_tests(RunOptions, build, body) -> Result<R, AutomationError>` (section 5) |

**An application's own harness** (no C# counterpart; MatterCAD's `TestHarness`)

| Rust | What it is |
|---|---|
| `AutomationRunner::attach(driver: HeadlessDriver, config: AutomationConfig) -> AutomationRunner` | A runner over a driver the caller owns, on the caller's thread: never cancelled, no close phase, no UI-panic capture (panics reach the thread's own unhandled handler). `driver()`/`driver_mut()`/`app()`/`app_mut()` reach inside; `into_driver(self) -> HeadlessDriver` hands the driver back. |
| `HeadlessDriver::with_painter(self, Box<dyn HeadlessPainter>) -> Self` | Paint frames through the application's renderer instead of the default `SoftwarePainter`. `HeadlessPainter` is `paint(&mut self, &mut App, width, height)`, `resize(&mut self, width, height)` (default no-op), `screen(&self) -> Option<&Framebuffer>` (pixels kept in memory) and `capture(&self) -> Option<Framebuffer>` (pixels read out on demand, at most once per painted frame). MatterCAD wraps agg-gui-wgpu's `HeadlessFrame`, or a painter with no pixels without a GPU adapter. |
| `HeadlessDriver::set_background_busy(&mut self, Box<dyn Fn(&App) -> bool>)`, `clear_background_busy`, `background_busy() -> bool`, `is_idle() -> bool` | Work running off the UI thread (compute workers) keeps the driver from being idle. |
| `AutomationRunner::wait_for_idle(max_seconds) -> bool` | Pump frames until a frame that did not paint leaves the driver idle (no background work, no due idle work, no draw requested), or until `max_seconds` of UI time has passed. `wait_for_pending_ui_work` also keeps pumping, up to its `max_milliseconds`, while background work runs. |

**Supporting types**
- `InputMethod` (= `IInputMethod`) gains `current_screen` (slice 31: the live window's read-back).

## 4. How simulated input gets in

**Rule:** the runner never calls widget `on_event` directly. Everything goes through the code shells use, so nothing that hover, capture, focus, modals, tooltips or the on-screen keyboard do can be bypassed.

1. **`agg_gui::shell_input::InputForwarder`** owns the shell-neutral bookkeeping (cursor, held buttons and modifiers, click counting by `ClickPolicy` or `ClickCount::Explicit`, the real-input gate via `InputSource::Platform`/`Simulated`) and calls the `App::on_*` entry points; agg-gui-shell and the web shell feed it `ForwarderEvent`s. `SimulatedInput` produces the same `ForwarderEvent`s (`forwarder.simulated(...)`), so headless and live clicks run identical code.
2. **Headless:** `HeadlessDriver` owns the `App` and a forwarder. Input is delivered immediately, then the runner pumps a frame where the operation calls for one (see section 5).
3. **Live:** `LiveDriver` holds an `agg_gui_shell::ShellSession` (new; see section 7) and feeds the same forwarder that its winit handler uses. Real winit input is dropped while `set_platform_input_enabled(false)` is in effect; that is the `RealInputIgnored` equivalent. `DesktopDeactivationIgnored` maps to `set_platform_deactivation_enabled(false)`.
4. **Order of events:** C# queues simulated input on RunOnIdle, so it runs FIFO with other idle actions. The Rust shells dispatch OS input before the frame's idle drain, and the runner follows the shells (the product path), not that C# simulation artifact. This ordering is pinned by a runner test.

## 5. Threading and timing

**What C# does.** The window's message loop owns the calling thread. The test body runs on a pool thread, and every gesture posts to RunOnIdle and blocks on a reset event or a draw. `Task.WhenAny` races the test budget (which starts at Load), the body and any UI-thread exception.

**What Rust does: the test drives the pump.**
- `App`, widgets and agg-gui's thread-locals can't cross threads, so the test body runs on the UI thread.
- Every runner primitive is synchronous. It injects input, then pumps frames until C#'s wait condition holds. Because the test thread is the UI thread, "the UI has taken the event" is guaranteed when the call returns. That is a stronger guarantee than C# gives; none of C#'s ceiling-bounded waits can expire too early.

**`show_window_and_execute_tests`:**
```rust
pub fn show_window_and_execute_tests<S, R>(
    opts: RunOptions,
    build: impl FnOnce() -> (AutomationWindow, S) + Send + 'static,   // runs on the test thread
    body:  impl FnOnce(&mut AutomationRunner, &S) -> R + Send + 'static,
) -> Result<R, AutomationError> where R: Send + 'static
```
- It spawns one fresh named thread per run (named `<<< UI THREAD`).
- **Isolation:** a fresh thread means fresh thread-locals (focus, modifiers, tooltip, animation and idle queue). That replaces C#'s `[NotInParallel]` plus `ResetForTests`/`Keyboard.Clear`, so tests can run in parallel. The one exception is process-global state: the platform has a thread-local override (`platform::override_platform_for_thread`); the input profile has none yet.
- **What runs on the thread:**
  1. `build()` creates the `AutomationWindow` (root widget, logical size, `on_load`, `on_close_requested` with its veto, `on_closed`) and the body's state.
  2. The driver comes up and the first paint fires `on_load` (= Load).
  3. A Loaded message is sent and `body` runs inside `catch_unwind`.
  4. If `require_test_completion` is set and `mark_test_complete()` was not called, the run fails with C#'s message.
  5. The body's outcome is sent (that stops the test clock), then the close phase runs (below) and reports how the window closed. A close the body asked for (`UiDriver::request_close`, which typing `%{F4}` calls) is the request the close phase answers.
- **The calling thread is the watchdog:**
  - `recv_timeout(max(budget, 30 s))` waits for Loaded. This is the bring-up budget. Its load watchdog reports on stderr 2 s before the end unless `timeout_is_the_expected_outcome` (slice 30c adds the stack dump).
  - It then waits `secs_to_test_failure` (wall clock, starting at Loaded) for the body's outcome.
  - On timeout it sets the run's `cancel: Arc<AtomicBool>`; the next runner call on the stuck thread panics with "test timed out". It returns `AutomationError::Timeout` and lets the thread finish detached, because Rust cannot kill a thread. This reproduces `AutomationRunnerTimeoutTest`: a body that sleeps 10 s still returns Timeout at about 1 s.
- **Close phase** (`execute/close.rs`):
  - A window with a `close_window` callback (C#'s `closeWindow`, MatterCAD's shutdown) hands it the runner instead of asking; the close budget starts when it returns (three budgets if it never does), and a panicking callback fails the run (after the body's own failure, before a close timeout; C# swallowed it) and the window is then asked plainly.
  - Otherwise it asks the window to close (C# `CloseOnIdle`), after one frame drains the UI work queued before the request; each request the window takes goes to `on_close_requested`, whose `false` is a veto. It keeps pumping frames, paced to the frame interval, while the app's own shutdown runs.
  - After `close_window_timeout` (wall clock) it force-closes (`on_closed`, then the tree is dropped) and the run fails with `AutomationError::CloseTimeout`. The watching thread gives up after twice the budget if the UI thread never comes back from a frame.
  - A panic while closing fails the run as `BodyPanicked("the window panicked while closing: …")`, ranked after the body's own failure.
  - Errors are ranked as C# ranks them: Timeout, then a UI panic while the body ran (`UiThreadPanicked`), then body panic, then a panicking `close_window` callback, then a panic while closing (a UI panic during the close counts; C# lost it), then CloseTimeout, then missing MarkTestComplete.
  - Still to come: the stack dump on a forced close (slice 30c).
- **UI-thread exceptions** (`UiThread.UnhandledException`):
  - idle actions run under `catch_unwind` (`agg_gui::ui_thread`)
  - a panic in a widget's `paint`, `paint_overlay`, `finish_paint` or `paint_global_overlay` is caught at that callback (`widget/paint/contain.rs`), reported with `UnhandledOrigin::Paint`, and the context's `save`/layer levels (`DrawCtx::paint_nesting` / `unwind_paint_nesting`) and the paint clip stack are put back, so the rest of the tree still paints and a widget that panics every frame leaks nothing; with no handler on the thread the panic is re-raised, as for idle actions
  - both are reported through `agg_gui::report_unhandled`.

  The run installs a handler on its UI thread (`set_unhandled_handler`) that keeps the first report; the body's next runner call unwinds (`UI_THREAD_FAULTED`), and the run fails with `AutomationError::UiThreadPanicked`.

**Frames.** One pumped frame (`HeadlessDriver::pump`) matches the shells' reactive tick:
1. advance the virtual clock by one frame interval
2. drain `ui_thread` (the idle queue)
3. if `app.wants_draw()` or a draw is forced: lay out when the layout key (size, scale, invalidation epoch) changed or layout was requested, then paint.

Headless paints through the driver's `HeadlessPainter`: a software `Framebuffer` (`SoftwarePainter`) by default, so paint is exercised and `get_current_screen` works; an application plugs in its own renderer with `with_painter` (section 3). The policy is `agg_gui::frame_policy` (`LayoutKey`, `LayoutTracker::tick` with `TickMode::Reactive`/`Forced`, `wants_frame`), which the native and web shells already use; the headless driver uses it too.

**Clocks**
- **`agg_gui::clock`** is a thread-local UI clock, real by default or virtual (`scoped_virtual`/`set_virtual`, `advance`), behind every behavioural time read in agg-gui and the node editor.
- **Headless default (`ClockPolicy::Virtual`):** each pumped frame advances `frame_interval` = 10 ms, the RunOnIdle tick C# cites. `ClockPolicy::Real` is available for tests whose background workers run in real time (MatterCAD's `with_compute_workers`).
- **Live:** the real clock always.
- **Several drivers on one thread** (MatterCAD's restart tests hold two harnesses) share the thread's clock. The first virtual driver puts the thread on the virtual clock, later ones join it, and the clock the thread had before comes back when the last of them drops, in any order; each virtual driver's frame advances the shared clock. Drivers alive on one thread must use the same `ClockPolicy`: a real-clock driver beside a virtual one would wait on time only the other advances, so creating one panics.
- **The UI-thread mark.** A driver marks its thread as a UI thread and never unbinds it: it cannot know whether the thread was a UI thread before it or still serves another driver. The thread's owner calls `ui_thread::unbind_current_thread` once its last driver is gone (a test harness at the end of its test, so a main queue it adopted passes to the next test at once). `show_window_and_execute_tests` needs none: its thread ends with the run.
- **Background work.** `HeadlessDriver::set_background_busy` takes a hook that reports work running off the UI thread; while it reports true, `wait_for_idle` and `wait_for_pending_ui_work` keep pumping frames (up to their ceilings). The other waits pump frames anyway, and their answers stay the condition's.

**How each C# wait maps to frames:**

The waits (`Delay`, `WaitForPendingUiWork`, `WaitFor`/`Assert`, `WaitforDraw`, and the name polls) map to frames as `runner/waits.rs` documents, the pointer's pacing as `runner/pointer.rs` does, and typing as `runner/keyboard.rs` does.

In live mode each of these pumps `pump_app_events` until the condition holds or the real deadline passes.

**Double clicks.** C# needs the two downs within 550 ms of each other. That is unaffected in headless (virtual time) and handled in live mode by the explicit click count.

## 6. Name lookup and search regions

**Names**
- `id()` **is** C#'s `Name` (`WidgetBase.name` by default; `Window` reports its title). A widget without a `WidgetBase` takes a name by wrapping it in `agg_gui::widgets::Named` (one child at its natural size, the child's min/max size, optional `with_properties`); it does not pass the child's anchors or margin on.

**Non-widget named targets** (C#'s `FindDescendants` overrides plus `NamedObject`/`OffsetHint`), which exist:
- `Widget::find_named_targets(&self, name: &str, out: &mut Vec<NamedTarget>)`, a default-empty trait method: a widget pushes its own targets named `name` that are on show (children are walked by the caller).
- `agg_gui::NamedTarget { rect_local: Rect, enabled: bool, object: Option<Rc<dyn Any>> }` (widget-local, Y-up; may lie outside the widget's bounds), with `new`, `with_enabled`, `with_object`, `object_as::<T>()`.
- Implemented by:
  - `ComboBox`: the open list's rows in view, named `"{text} Menu Item"` (C#'s `DropDownList.AddItem`) or by `set_item_name`/`with_item_name`, with the row's enabled state and its index (`usize`) as object.
  - `MenuBar`: its open menu's rows through `PopupMenu::named_row_targets(name, viewport, out)` (any host of a `PopupMenu` can call it), with the item's enabled flag and its path (`Vec<usize>`) as object; the titles are child widgets named `"{label} Menu"`. A row's name is `menu_row_name(item)`: the item's `automation_name` when set (`MenuItem::with_automation_name`, C#'s `MenuItemModel.AutomationName`), else `"{label} Menu Item"` (C#'s `PopupMenu`).
  - The right-click menus agg-gui owns: `TextField`, `TextArea` and `RichTextEdit` (through `TextContextMenu::named_row_targets(name, out)`) and `MarkdownView` report their open menu's rows the same way (`"Cut Menu Item"`, `"Select All Menu Item"`, ...).
  - MatterCAD's own hosts: its View3D reports the Object3D controls and scene children, and its popup and context-menu hosts report their rows.

**The tree walk** (`tree_query::find_named_hits`, used by `runner/named.rs`) puts a widget's targets before the widget itself, as C#'s overrides add them before calling the base. A target hit's `offset_hint` is its rect's center. A target is on show when its widget and every ancestor are visible and the target's own rect reaches into the window. The widget's own area is not asked, nor are ancestor clips: drop-down and menu rows paint in the overlay pass, and a context menu's host may have no size at all. Search regions and `get_region_by_name` use the target's own rect, and `wait_for_widget_enabled` on a target's name waits for the target's own `enabled` flag (read afresh each look) as well as its widget's enabled chain.

**Where a widget is, and what of it shows** (`paint_geometry`, under `tree_query::placement`): the lookups replay painting's transform and clip chain, so a widget is found where the frame draws it. A child is offset by its bounds origin, rounded when it enforces integer bounds, and such a widget's transform is floored onto the pixel grid (`snap_to_pixel`; exact at device scale 1). Children are clipped to `clip_children_rect`, then offset by the child transform, and each child by its parent's `child_paint_clip` (a `FlexRow` slot). A subtree that paints in the overlay pass (`defer_paint_to_overlay`, a modal inside a clipped container) escapes its ancestors' clips and is clipped by the window alone.

**Choosing among same-named things, and where to click** (`name_pick`, pure functions of a tree and the window size, shared by the runner's lookups and an application's own harness):
1. Only what shows counts (`HitPlacement::shows`).
2. Those a press can reach come first (C# `PreferReachable`): the click point shows and no ancestor's `hit_test` refuses it (`pointer_reach::can_reach`); all count when none can be reached.
3. The largest visible area wins, the first in tree order on a tie, with areas within a relative `AREA_TOLERANCE` (1e-9) counting as equal: equal widgets at different offsets come out a few ulps apart in window space, and C#'s strict comparison let that noise pick the second of equals.
4. The click lands at the centre (C# `MoveMouseToWidget`), or at the centre of the visible part when a clip hides the centre (a row half scrolled out of its list): C# pressed whatever lay over the hidden centre of a widget its lookup had just required to show.

**A target that moves while the pointer travels** (a part in a 3D view the camera is still framing): a click by name re-aims at every step of its move, as C#'s `MoveMouseToWidget` re-reads the widget's rectangle. For a named target inside a widget, `AutomationRunner::refind_target(name, &hit)` looks the target up again among those the same widget reports under that name (the same object by `Rc::ptr_eq`, else the same rectangle, else the only one) and aims at the centre of what shows (rule 4); one it can no longer tell apart keeps its last aim. A click presses `ClickOpts::button` (Left by default; `MouseButton::Other(n)` presses an extra button such as a mouse's back button, delivered as itself through `MouseAction::OtherDown`/`OtherUp`).

Rules 3 and 4 are the runner's behaviour for every consumer, not a hook: both make a lookup keep its own contract (the first of equals; a found widget is a clickable one). `find_on_screen(root, name, viewport)` applies all four in one look; `AutomationRunner::find_now(name)` / `find_now_in(name, region)` is the same from a runner, taking `&self` and pumping nothing, so a `wait_until` predicate can ask it. `hit_placement` and `aim_point` report a hit's rectangles and its click point (logical, Y-up).

**Image searches** use `region.image`, or a fresh whole-window capture.

**Reading widget state from tests** (C# reads `field.Text`, `IsOpen`, `ContainsFocus` straight off objects):
- Test helpers (over `Widget::as_any`/`as_any_mut`):
  - `runner.with_widget::<TextField, _>("field", |f| f.text())`
  - `runner.with_handle_mut(...)`
  - `runner.contains_focus(&handle)`, which needs a new public `App::focused_path()`
  - `runner.app()` / `app_mut()` for raw entry points, which C#'s WidgetClickTests use via `testWindow.OnMouseDown`.
- `properties()` keys stay available as a fallback.
- `Parents<T>()` / `Children<T>()` become `tree_query::parents(&handle)` / `children(&handle)`.

## 7. Dialog provider, live shell, diagnostics

**`AutomationDialogProvider`** (exists, `src/dialog/`):
- agg-gui has no file-dialog abstraction; MatterCAD has `FileService`. `AutomationFileDialog::show(presenter: &dyn ModalPresenter, kind: FileDialogKind, callback: impl FnOnce(FileDialogResult) + 'static)` builds an in-canvas `ModalSheet` (C#'s 600 × 200) with C#'s warning label and a `TextField` named **"Automation Dialog TextEdit"** (`AUTOMATION_DIALOG_TEXT_EDIT`), focused through `focus::request_focus` as it shows.
- Enter closes it (the field's `on_enter`, or the sheet's default action when the field is unfocused). `parse_dialog_text` splits on `;` and trims quotes, giving `FileDialogResult { kind, file_name, file_names }`; text of length 2 or less (UTF-16 units) is "cancelled", as in C#. Escape closes it cancelled. The callback runs on idle: it waits in a UI-thread-local queue and a `ui_thread::run_on_idle` action posted from that thread runs it, so it need not be `Send`.
- `FileDialogKind` is `Open` / `Save`. `SelectFolderDialog` (NotImplemented in C#) is left out until a test needs it. `resolve_file_path` passes the path through; `show_file_in_folder` does nothing.
- `ModalPresenter { fn present(&self, modal: Box<dyn Widget>) }`: the modal hides itself when done and the presenter drops it. `AutomationWindow` implements it, and `AutomationWindow::modal_presenter()` hands out a cloneable `AutomationModalPresenter`; either puts the built-in overlay layer (`dialog/overlay.rs`) above the root at bring-up, so a window that never asks keeps its tree as built. MatterCAD implements it with its `popup_layer`.
- MatterCAD adds `AutomationFileService: FileService` that opens this dialog. C#'s `CompleteDialog` is `AutomationRunner::complete_dialog(text)` (`runner/dialog.rs`): find the field within 5 s (C# `RequireWidget`'s message otherwise), wait up to 5 s for it to contain focus, `type_text(text)`, `{Enter}`, wait up to 5 s for it to disappear.

**Live shell**
- **New in `agg-gui-shell`: `ShellSession`.** It does the same window and GPU bring-up as `run`, but returns control to the caller:
  - `pump(timeout)` via winit 0.30's `EventLoopExtPumpEvents::pump_app_events` (Windows, macOS, Linux)
  - `app()` / `app_mut()`, `forwarder()`
  - `request_close()` (runs the host's `on_close_requested`), `force_close()`, `presented_frames()`
  - `set_platform_input_enabled`, `set_platform_deactivation_enabled`.
- `run()` becomes a thin loop over the same internals.
- Live tests are separate `harness = false` binaries (macOS needs the event loop on the main thread). The budget watchdog is a helper thread: it sets `cancel`, and after a grace period it dumps stacks and calls `process::exit`.
- The web shell is out of scope for v1, as it is in C# (wasm can't block). The `UiDriver` trait leaves room for a later async front.

**`StartupFailureLog`** (exists, `src/startup_failure_log.rs`):
- process-wide `Mutex` state; `last_failure_description()`
- `record(phase, &dyn Error)` (display text plus `source()` chain) and `record_panic(phase, payload)`, each with the backtrace at the record site; `reset()`, `append_to(msg)`
- an immediate write to `std::io::stderr()`, which libtest's capture (the `print!` family only) does not intercept; `set_immediate_sink` replaces it for tests
- the time stamp is UTC `HH:mm:ss.fffZ` (C# used local time; there is no time-zone database here).

**`ThreadStackDump`** (`stack-dump` feature). Same design as C#: capture a dump in-process, then walk it.
- `minidump-writer` writes the current process from a capture thread; `minidump-processor`/`minidump-unwind` walk it with local symbols.
- Also ported: `CaptureAttempts` = 5 with retry pause, `MaxFramesPerThread`, `register_current_thread(label)`, rejection of a dump that cannot walk the capturing thread, and `write_to_console`.
- `run_dump_writer(program, args, log_path, timeout)` is the external-writer fallback. It logs to a file (never a pipe) and kills the writer at the timeout.
- Two C# cases are Mach-O-specific and port as minidump equivalents:
  - truncated LC_THREAD → a truncated thread-list stream is not read
  - duplicate-stack-pointer repair → two threads on one SP still report.

## 8. Gaps to fill first, and the test port map

**agg-gui gaps** (each lands test-first in the slice listed in section 9):

| # | Gap | Slice |
|---|---|---|
| G12 | Flex reverse directions (`FlexRow`/`FlexColumn` lay children out right-to-left / bottom-to-top, as `FlowDirection.RightToLeft`/`BottomToTop`); neither widget has a reverse mode | 25 |
| G17 | `ShellSession` / pump API, input and deactivation gates | 31 |

**C# class → Rust file** (`agg-gui-automation/tests/`; tests keep their names in snake_case):

| C# class (count) | Rust file | Notes |
|---|---|---|
| AutomationRunnerTests (11) | `automation_runner_tests.rs` | via `show_window_and_execute_tests`; 1 to go (AnExpectedTimeoutLeavesNoThreadDump, slice 30c; ImageWaitsSearchTheGivenRegion, VetoedCloseIsForcedAndReportedAsFailure, StaticDelayExpires…, ZeroSecondWaitsReportWhatIsThereNow, WindowLoadTimeIsNotChargedToTheTestBudget, AutomationRunnerTimeoutTest, GetWidgetByNameTestNoRegionSingleWindow, GetWidgetByNameTestRegionSingleWindow, DoubleClickByNameSendsTwoFullClickPairsWithProductionClickCounts and TypeDeliversPunctuationIntoAMultiLineField are ported) |
| AutomationRunnerTests.Winforms (3) | `live/automation_runner_live_tests.rs` | `harness = false`, all desktop OSes |
| FlowLayoutTests (24) | agg-gui's `tests/flow_layout_agg_sharp/flow_layout_tests.rs` and `flow_layout_tests_anchors.rs` | FlowLayoutWidget → FlexColumn/FlexRow; image compares become box compares. 20 ported; the 4 reverse-direction tests (RightToLeftTests, NestedFlowWidgetsRightToLeftTests, RightLeftWithAnchorLeftRightChildTests, BottomTopWithAnchorBottomTopChildTests) wait on G12. `EnsureFlowLayoutMinSizeFitsChildrenMinSize` has no `[Test]` attribute in C# and is not counted. |
| MacTextEditKeyBindings (12) | `mac_text_edit_key_binding_tests.rs` | all ported |
| WidgetClickTests (3) | `widget_click_tests.rs` | all ported |
| MenuTests (1) | `menu_tests.rs` | DropDownList → ComboBox; ported |
| MouseInteractionTests (13) | `mouse_interaction_tests.rs` (+ `mouse_interaction/*.rs`) | all ported |
| PaintExceptionContainmentTests (2) | `paint_exception_containment_tests.rs` | both ported (headless) |
| PresentFailureContainmentTests (1) | `agg-gui-wgpu/tests/present_failure_containment_tests.rs` | headless wgpu device and a custom pass recording an invalid copy into the frame's own encoder; ported (passes trivially without a GPU adapter, as C# skips on a CPU host) |
| TextEditFocusTests (3) | `text_edit_focus_tests.rs` | all ported |
| TextEditTests (13) | `text_edit_tests.rs` (+ `text_edit/*.rs`) | TextEditWidget → TextField/TextArea; NumEdit → NumberEdit; all ported |
| ThreadStackDumpTests (10) | `thread_stack_dump_tests.rs` | `stack-dump` feature |
| ToolTipTests (7) | `tool_tip_tests.rs` (+ `tool_tip/*.rs`) | all ported |

## 9. Slice plan (about 25 minutes each, one agent; each ends with `cargo test` green)

| # | Slice | Tests that land |
|---|---|---|
| 25 | G12 (reverse flex directions), then the four reverse-direction Flow tests | RightToLeftTests, NestedFlowWidgetsRightToLeftTests, RightLeftWithAnchorLeftRightChildTests, BottomTopWithAnchorBottomTopChildTests |
| 30 | ThreadStackDump, part 1 (retry and dump-writer subprocess) | ACaptureThatLosesTheDumpRaceIsRetakenNotReparsed, ACaptureThatNeverSucceedsGivesUpAndReportsEveryAttempt, WritingADumpCapturesTheDumpWritersLogInsteadOfPrintingIt, ADumpWriterWithMoreOutputThanAPipeHoldsStillFinishes, ADumpWriterThatNeverExitsIsKilledAtTheTimeout |
| 30b | ThreadStackDump, part 2 (minidump capture and walk) | CaptureNamesTheCallingThreadsOwnFrames, ADumpThatCannotWalkItsCapturingThreadIsRejected, RegisteredThreadsAreLabelledInTheDump, ATruncatedThreadCommandIsNotRead, ADumpWithTwoThreadsOnOneStackPointerStillReports |
| 30c | Load and close watchdogs write dumps to `TestResults/` | AnExpectedTimeoutLeavesNoThreadDump |
| 31 | G17 `ShellSession` plus `LiveDriver`, with the simulated-mouse overlay (`RunOptions.draw_simulated_mouse`, `overlay::render_mouse`) | RealMouseInputDoesNotReachAWindowUnderAutomation |
| 32 | Live pump robustness | IdlePumpSurvivesAnotherWindowsTeardown, ShowFromNonPumpThreadReturnsToItsCaller |

**Tally** (18 still to port): AutomationRunnerTests 11 (10 ported; AnExpectedTimeoutLeavesNoThreadDump to go), Winforms 3, Flow 24 (20 ported), Mac 12 (12 ported), Menu 1 (1 ported), Mouse 13 (13 ported), Paint 2 (2 ported), Present 1 (1 ported), TextEditFocus 3 (3 ported), TextEdit 13 (13 ported), ThreadStackDump 10, ToolTip 7 (7 ported), WidgetClick 3 (3 ported): **103**.

**Moving `mattercad-app-test` onto the runner** (the harness, `run_test`, the file dialog and the name lookups are done: section 1)

| # | Slice | Notes |
|---|---|---|
| M5-M9 | Move the test files still on the old harness helpers to the runner API, about 3 files per slice | In `crates/mattercad-app-test/tests`, 63 files still call `h.click(`, 12 `h.type_keys(`, 13 `h.drag(`, 36 `h.click_by_name(`, 61 `h.wait_for(` and 43 `h.wait_for_name(` (the harness versions in `automation.rs`/`harness_input.rs`); `click_by_name`/`drag_drop_by_name`/`type_text`/`wait_for_name`/`assert` on the runner replace them; frame-counted waits become C#'s seconds; teleporting clicks become stepped moves (fix whatever this exposes, test-first). |
| M10 | Remove the old helpers | Delete the harness's `click`, `type_keys`, `drag`, `click_by_name`/`wait_for*` and `MAX_WAIT_FRAMES` once no test uses them; update crate docs. That leaves 47 of the 61 C# AutomationTests files, which can then be ported one per slice with the same API. |

## 10. Risks

- **Workers that serve a non-main UI thread** must wake it through that thread's queue (`ui_thread::current_queue()` captured on the UI thread, then `UiQueue::run_on_idle` / `UiQueue::signal_async_state_change`); an unbound worker's `animation::signal_async_state_change` wakes only the main queue's owner. MatterCAD's workers capture the queue this way, so a headless test on its own thread sees their wakeups; a new worker anywhere must do the same.
- **Virtual time vs. real worker threads.** Covered by `ClockPolicy::Real` and the driver's background-busy hook (`set_background_busy`), which `wait_for_idle` and `wait_for_pending_ui_work` honour, so `settle` keeps working. Idle detection reads the thread's queue through `ui_thread::count_expired`, which does not see work no drain has stamped yet; such work is posted with a draw request, which keeps `is_idle` false until the next frame drains it.
- **A timed-out body thread keeps running.** It only touches its own thread-locals, and it unwinds at its next runner call.
- **Process-global agg-gui state** (input profile, tilt, fullscreen). It needs thread-local overrides, as the platform has, or `serial_test` for the few tests that touch it.
- **Headless software paint can be slow on MatterCAD's tree.** Frames paint only when `wants_draw`, so idle `delay`/`wait_for` frames are cheap.
- **The `^` → command-modifier choice** differs from C#'s literal Control on macOS. It is deliberate; `rust_only_caret_is_the_platform_command_modifier` pins it per platform.

### Critical files for implementation
- /Users/larsbrubaker/Development/MatterCAD/Submodules/agg-sharp/GuiAutomation/AutomationRunner.cs
- /Users/larsbrubaker/Development/rust-apps/agg-gui/agg-gui/src/widget/app/pointer.rs (with `app.rs`, `app/keyboard.rs`, `app/path_anchor.rs`)
- /Users/larsbrubaker/Development/rust-apps/agg-gui/agg-gui/src/widget/tree_inspector.rs (`find_widget_by_id`, `find_widget_screen_rect`)
- /Users/larsbrubaker/Development/rust-apps/agg-gui/agg-gui-shell/src/shell_loop.rs (input forwarding, close veto, the pump refactor)
- /Users/larsbrubaker/Development/rust-apps/mattercad-rust/crates/mattercad-app-test/src/lib.rs and src/automation.rs (frame policy and helpers to migrate)
