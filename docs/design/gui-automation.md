# GUI Automation for agg-gui: porting agg-sharp's GuiAutomation (design)

*Design for the port of agg-sharp's GuiAutomation (MatterCAD's `Submodules/agg-sharp/GuiAutomation`). Open work only: delete sections as slices land.*

## 0. Summary

- **New crate:** `agg-gui-automation` in the agg-gui workspace, the counterpart of C#'s separate `GuiAutomation` assembly.
- **The simulated mouse is real input.** Mouse and keyboard go through the same `App::on_*` entry points the native and web shells call. The mouse moves in `MouseMoveSteps` (5) eased steps (Cubic.Out). Presses and releases follow C#'s exact event shapes, including explicit click counts.
- **Threading:** C# runs the test on one thread and the UI on another, joined by `RunOnIdle` and reset events. Rust instead runs the test body on the UI thread, and every runner call pumps frames itself.
  - **Headless:** `HeadlessDriver` owns the `App`, a software framebuffer and a virtual clock. It is deterministic and needs no window or GPU.
  - **Live window:** `LiveDriver` pumps a real `agg-gui-shell` window with winit's `pump_app_events`.
- **Two clocks:**
  - Virtual time: `Delay`, `WaitFor`, timeouts and pointer pacing.
  - Wall time: the hang watchdogs (test budget, bring-up, close).
- **One thread per test:** `show_window_and_execute_tests` runs each test on a fresh thread. That isolates agg-gui's thread-local state and lets the caller time out a stuck body, which is what C#'s `Task.WhenAny` does.
- **Order of work:** the remaining slices (17 onward) fill agg-gui's gaps test-first, port the 76 remaining Agg Automation Tests 1:1, then move `mattercad-app-test` onto the runner (section 9).

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

**Tests:** 13 files, 12 classes (`AutomationRunnerTests` is a partial class split across 2 files), 103 tests. Six of the 13 files never call the runner and drive widgets directly: TextEdit, Mac bindings, ThreadStackDump, most of MouseInteraction, ToolTip and FlowLayout. Those still need a headless window to port.

**agg-gui**
- `App` (`agg-gui/src/widget/app.rs` with `app/pointer.rs` and `app/keyboard.rs`) takes Y-down physical input:
  - `on_mouse_move`, `on_mouse_down`/`on_mouse_up(x, y, button, mods)`
  - `on_key_down`/`on_key_up(Key, mods)`, where `Key::Char` is the character (there is no separate KeyPress)
  - wheel, file drop and modifier calls.
- Tree lookup: `Widget::id()` (overridden by only a few agg-gui widgets), `find_widget_by_id`, `find_widget_screen_rect`.
- `path_anchor` tracks widget identity by heap address; `agg_gui::WidgetAnchor` exposes it (a path that follows its widget through reorders and reports it gone), with `widget::walk_path` and `App::focused_path`.
- Nearly all state is thread-local. Exceptions: `CURRENT_PLATFORM` and the input profile are process-global atomics.
- Widget names (`WidgetBase.name`, `Widget::with_name`, default `id()`, `widgets::Named`), typed downcasts (`Widget::as_any`/`as_any_mut` on the core widgets) and an origin-placing container (`widgets::AbsoluteLayout` over `WidgetBase.origin`) exist.
- `agg_gui::ui_thread` is the idle queue (`run_on_idle`, `run_on_idle_after`, intervals, `invoke_pending_actions`), one queue per UI thread, delays on `agg_gui::clock`, drained by both shells every loop iteration. Frame-loop wakeups are counted per queue: a post or `signal_async_state_change` wakes the thread that drains that queue (an unbound worker's, the main queue's owner), never another UI thread. Queued work runs under panic containment and reports through `agg_gui::report_unhandled` (`agg_gui::unhandled`, a per-thread handler set with `set_unhandled_handler`; with none, the first panic is re-raised after the rest of the drain).
- `agg_gui::frame_policy` holds the frame policy both shells use: `LayoutKey` (size, device scale, invalidation epoch), `LayoutTracker` (needs-layout, `layout_if_needed`, and the GPU-free `tick` a headless driver runs), and `wants_frame` (the paint decision). `agg_gui::shell_input::InputForwarder` holds the input bookkeeping both shells feed their OS events through as `ForwarderEvent`s.
- `Widget::is_enabled` reports a widget's own enabled state (default true; `Button`, `SegmentedControl` and `ChevronWidget` report theirs).
- Click counts: `App::on_mouse_down_clicks` takes a stated count, the forwarder passes every press's count, and widgets read `event::current_click_count()` / `event::is_double_click()` (C#'s `Clicks` / `IsDoubleClick`); `MultiClickTracker` honours only a stated count.
- Under the mouse: the hovered chain and the widgets it covers (under a sibling drawn above them, `UnderMouseNotFirst`) send bounds `MouseEnter`/`MouseLeave`, the chain's deepest widget gets first-under-mouse `MouseOver`/`MouseOut` (C#'s `MouseEnter`/`MouseLeave`), a press updates them as a move does, capture follows the captured widget's own area and freezes everything else, a release over the capture holder sends no hover-refresh move, and `App::hovered_chain`/`first_under_mouse`/`under_mouse_state`/`captured_path` plus the thread snapshot `agg_gui::under_mouse_state_of(WidgetId)` answer C#'s `UnderMouseState`, `MouseCaptured` and `ChildHasMouseCaptured`.
- `agg_gui::observe_events(WidgetId, callback)` watches the events one widget receives (C#'s per-widget `MouseMove +=` and friends) without wrapping it.
- `widgets::RadioButton` is C#'s standalone `RadioButton`: a widget per option, exclusive with the radio buttons in its parent (settled by the dispatch walk before its callbacks run), individually disableable, with `on_checked_state_changed`/`on_click` and `check_radio_child` for a programmatic check.
- **Missing piece:** paint-panic containment.

**`mattercad-app-test`**
- `TestHarness` builds the real tree headless, sends input straight to `App::on_*`, and keeps its own copy of the shells' frame policy (`input_frame`/`shell_frame`/layout key) instead of `agg_gui::frame_policy` (M2 replaces it).
- Clicks teleport the pointer: there is no stepped move.
- Waits count frames (`MAX_WAIT_FRAMES = 120`) instead of seconds.
- Typing is ad hoc: `type_keys` handles only 4 `{}` tokens and a leading `^`.
- Names of non-widget targets (3D controls, scene objects, menu rows) are found by parsing `properties()` strings.

## 2. Where the code goes

**Crate `agg-gui/agg-gui-automation`** (workspace member; consumers use it as a dev-dependency). It exists with `keys`, `key_mapping`, `typed_key_parser`, `search_region`, `waits` (`static_delay`), `driver` (`HeadlessDriver`, `HeadlessWindow`, `ClockPolicy`, `FrameKind`, `UiDriver`), `probe` (`ProbeWidget`) and `tree_query` (`WidgetHandle`, `find_by_name`, `placement`/`screen_rect`/`clipped_rect`, `actually_visible_on_screen`, `parents`/`children` and their `_of_type` forms), `execute` (`show_window_and_execute_tests`, `RunOptions`, `AutomationWindow`, `AutomationError`), `pointer_reach` (`can_reach`, `prefer_reachable`), `pointer_state` (`under_mouse_state`, `mouse_captured`, `child_has_mouse_captured`, `focused` for a handle), `input` (`InputMethod`, `SimulatedInput`, `Point2D`, `MouseAction`) and `runner` (`AutomationRunner` with `AutomationConfig`, `mark_test_complete`, driver access, the waits in `runner/waits.rs`, the name lookups in `runner/named.rs`, the pointer gestures in `runner/pointer.rs`, the drags in `runner/drag.rs` and the keyboard (`type_text`, `ModifierKeys`, `press_modifier_keys`/`release_modifier_keys`, `select_all`/`select_none`) in `runner/keyboard.rs`); the other modules and members below are still to come.
- It keeps test-only code (watchdogs, stack dumps, image matching) out of product builds.
- Its optional live mode depends on `agg-gui-shell` (winit and wgpu), which core `agg-gui` must not.
- Features, each added with the slice whose code uses it (none exist yet; headless is the default):
  - `live = ["dep:agg-gui-shell"]` (slice 31)
  - `stack-dump = ["dep:minidump-writer", "dep:minidump-processor", "dep:minidump-unwind"]` (slice 30)

Modules (each under 800 lines, each opening with a purpose comment):

```
src/lib.rs                 crate docs, re-exports
src/runner/mod.rs          (exists) AutomationRunner, AutomationConfig, MarkTestComplete
src/runner/named.rs        (exists) Get*/Wait*/NameExists/NamedWidgetExists/ChildExists/GetRegionByName; GetObjectByName, ScrollIntoView to come
src/runner/pointer.rs      (exists) stepped moves, ClickOrigin/ClickOpts, Click*/DoubleClickByName/RightClick*/MoveToByName/SetMouseCursorPosition
src/runner/drag.rs         (exists) DragOpts/DragDropOpts, Drag*/Drop*
src/runner/keyboard.rs     (exists) Type, ModifierKeys, Press/ReleaseModifierKeys, SelectAll/None
src/runner/waits.rs        (exists) Delay, WaitFor/WaitUntil, Assert, WaitForPendingUiWork, WaitforDraw
src/runner/images.rs       ClickImage/DragImage/DropImage/ImageExists/WaitForImage, GetCurrentScreen
src/input.rs               (exists) InputMethod trait (IInputMethod) + SimulatedInput (AggInputMethods), MouseConsts→MouseAction, keyboard members; current_screen to come
src/pointer_reach.rs       (exists) PointerReach
src/tree_query.rs          (exists) WidgetHandle, NamedHit (GetByNameResults; its `target` lands with G11), screen/clip rects, ActuallyVisibleOnScreen, inherited Enabled, Parents/Children
src/image_match.rs         FindLeastSquaresMatch over agg_gui::Framebuffer
src/driver/mod.rs          (exists) UiDriver trait, FrameKind, ClockPolicy
src/driver/headless.rs     (exists) HeadlessDriver
src/driver/window.rs       (exists) HeadlessWindow (the "SystemWindow" for tests that use no runner)
src/driver/live.rs         (feature live) LiveDriver over agg_gui_shell::ShellSession
src/execute/              (exists) show_window_and_execute_tests, RunOptions, AutomationWindow, AutomationError, load watchdog; close watchdog to come
src/probe.rs               (exists) ProbeWidget: generic C#-GuiWidget stand-in (name, bounds, colour, event log/callbacks, C# Click semantics)
src/overlay.rs             RenderMouse (simulated pointer drawing)
src/dialog_provider.rs     AutomationFileDialog (AutomationDialogProvider)
src/startup_failure_log.rs StartupFailureLog
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
| `CloseWindowTimeoutSeconds` (15) | `RunOptions.close_window_timeout` |
| `InterpolationType` (unused) | Dropped as dead code; noted in the port comment. |
| `GetCurrentScreen()` | `get_current_screen() -> Framebuffer`: headless, the last software frame; live, a read-back. |
| `RenderMouse` | `overlay::render_mouse`. The driver draws it after `App::paint`: a circle, green while the left button is down, "S"/"C" for held modifiers, plus the click count. |
| `Dispose`, `KeyDown`/`KeyUp` (throw NotImplemented in C#) | `Drop`. The two key methods are left out. |

**Name lookup** (the rest of it exists in `runner/named.rs`)

| C# | Rust |
|---|---|
| `GetByNameResults.NamedObject` | `NamedHit` gains `target: Option<NamedTarget>` with G11 |
| `GetObjectByName` | `get_object_by_name(...) -> Option<NamedHit>`: the non-widget `NamedTarget` |
| `SetTarget` (`DebugShowBounds`) | agg-gui has no bounds overlay; `get_widget_by_name` flashes nothing until one exists |
| `ScrollIntoView(name, amount)` | `scroll_into_view(name)`: calls `Widget::scroll_rect_into_view` on the nearest scrollable ancestor, falling back to wheel notches |
| `WidgetNotFoundMessage` | `widget_not_found_message(op, name)` exists; it appends `StartupFailureLog` once that lands (slice 29) |

**Images**

| C# | Rust |
|---|---|
| `ClickImage`, `DragImage`, `DropImage`, `DragDropImage`, `ImageExists`, `WaitForImage` (needle as file name or image) | Same names, needle as `ImageNeedle::Path` or `ImageNeedle::Buffer`; least-squares match in `image_match.rs` |
| `DoubleClickImage`, `MoveToImage` (throw NotImplemented in C#) | Left out (no stubs); the port comment names them. |

**Run entry point**

| C# | Rust |
|---|---|
| `ShowWindowAndExecuteTests(window, test, secs = 30, images, closeWindow, timeoutIsTheExpectedOutcome)` | `show_window_and_execute_tests(RunOptions, build, body) -> Result<R, AutomationError>` (section 5) |

**Supporting types**
- `SearchRegion::image(capture)` takes the runner's `get_current_screen` as its capture function.
- `InputMethod` (= `IInputMethod`) gains `current_screen` (slice 23).

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
- **Isolation:** a fresh thread means fresh thread-locals (focus, modifiers, tooltip, animation and idle queue). That replaces C#'s `[NotInParallel]` plus `ResetForTests`/`Keyboard.Clear`, so tests can run in parallel. The one exception is process-global state (`platform`, input profile); slice 21 adds a thread-local override for it.
- **What runs on the thread:**
  1. `build()` creates the `AutomationWindow` (root widget, logical size, `on_load`; slice 28 adds `on_close_requested`, the veto, and `on_closed`) and the body's state.
  2. The driver comes up and the first paint fires `on_load` (= Load).
  3. A Loaded message is sent and `body` runs inside `catch_unwind`.
  4. If `require_test_completion` is set and `mark_test_complete()` was not called, the run fails with C#'s message.
  5. The body's outcome is sent (that stops the test clock), then the close phase runs (below) and reports Closed. Today the close phase drops the tree on its thread, and a close the body asks for (`UiDriver::request_close`, which typing `%{F4}` calls) is only recorded (`HeadlessDriver::close_requested`); slice 28 brings the protocol below and makes that request start it.
- **The calling thread is the watchdog:**
  - `recv_timeout(max(budget, 30 s))` waits for Loaded. This is the bring-up budget. Its load watchdog reports on stderr 2 s before the end unless `timeout_is_the_expected_outcome` (slice 30c adds the stack dump).
  - It then waits `secs_to_test_failure` (wall clock, starting at Loaded) for the body's outcome.
  - On timeout it sets the run's `cancel: Arc<AtomicBool>`; the next runner call on the stuck thread panics with "test timed out". It returns `AutomationError::Timeout` and lets the thread finish detached, because Rust cannot kill a thread. This reproduces `AutomationRunnerTimeoutTest`: a body that sleeps 10 s still returns Timeout at about 1 s.
- **Close phase:**
  - It calls `opts.close_window` (the `closeWindow` callback) or asks the window to close.
  - The `on_close_requested` veto is honoured. It keeps pumping while the app's own shutdown runs.
  - The budget starts when the callback returns, with a 3× cap if it never does.
  - After the budget it force-closes (drops the tree), dumps stacks, and the run fails with `AutomationError::CloseTimeout`, wording as in C#.
  - Errors are ranked as C# ranks them: Timeout, then UI panic, then body panic, then CloseTimeout, then missing MarkTestComplete.
- **UI-thread exceptions** (`UiThread.UnhandledException`):
  - idle actions run under `catch_unwind` (`agg_gui::ui_thread`)
  - paint panics are contained per subtree (slice 27)
  - both are reported through `agg_gui::report_unhandled`.

  The runner captures the first report, fails the run with it, and starts the close.

**Frames.** One pumped frame (`HeadlessDriver::pump`) matches the shells' reactive tick:
1. advance the virtual clock by one frame interval
2. drain `ui_thread` (the idle queue)
3. if `app.wants_draw()` or a draw is forced: lay out when the layout key (size, scale, invalidation epoch) changed or layout was requested, then paint.

Headless paints into a software `Framebuffer`, so paint is exercised and `get_current_screen` works. The policy is `agg_gui::frame_policy` (`LayoutKey`, `LayoutTracker::tick` with `TickMode::Reactive`/`Forced`, `wants_frame`), which the native and web shells already use; the headless driver uses it too.

**Clocks**
- **`agg_gui::clock`** is a thread-local UI clock, real by default or virtual (`scoped_virtual`/`set_virtual`, `advance`), behind every behavioural time read in agg-gui and the node editor.
- **Headless default (`ClockPolicy::Virtual`):** each pumped frame advances `frame_interval` = 10 ms, the RunOnIdle tick C# cites. `ClockPolicy::Real` is available for tests whose background workers run in real time (MatterCAD's `with_compute_workers`).
- **Live:** the real clock always.

**How each C# wait maps to frames:**

The waits (`Delay`, `WaitForPendingUiWork`, `WaitFor`/`Assert`, `WaitforDraw`, and the name polls) map to frames as `runner/waits.rs` documents, the pointer's pacing as `runner/pointer.rs` does, and typing as `runner/keyboard.rs` does.

In live mode each of these pumps `pump_app_events` until the condition holds or the real deadline passes.

**Double clicks.** C# needs the two downs within 550 ms of each other. That is unaffected in headless (virtual time) and handled in live mode by the explicit click count.

## 6. Name lookup and search regions

**Names**
- `id()` **is** C#'s `Name` (`WidgetBase.name` by default; `Window` reports its title). MatterCAD's three ad-hoc `Named` structs move to `agg_gui::widgets::Named` in slice M3.

**Non-widget named targets** (C#'s `FindDescendants` override plus `NamedObject`/`OffsetHint`)
- New in agg-gui: `Widget::find_named_targets(&self, name, out: &mut Vec<NamedTarget>)`, defaulting to nothing.
- `NamedTarget { rect_local: Rect, enabled: bool, object: Option<Rc<dyn Any>> }`.
- Implemented by:
  - ComboBox popup rows and menu bar/popup rows (MenuTests clicks "item1" by name)
  - later, MatterCAD's 3D view (Object3D controls and scene children), replacing `CONTROL_KEYS`/`scene_objects` string parsing.

**The tree walk** (`tree_query`, used by `runner/named.rs`) gains the named targets with G11: a `NamedTarget`'s `offset_hint` is its rect's center.

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

**`AutomationDialogProvider`:**
- agg-gui has no file-dialog abstraction; MatterCAD has `FileService`. The automation crate provides `AutomationFileDialog::show(presenter, kind, callback)`. It builds an in-canvas modal (agg-gui `ModalSheet`) with C#'s warning label and a `TextField` named **"Automation Dialog TextEdit"**, focused on load.
- Enter closes it. The text is split on `;` and quotes are trimmed, giving `file_name` and `file_names`. Text of length 2 or less is treated as "cancelled", as in C#. The callback is posted with `ui_thread::run_on_idle`.
- `SelectFolderDialog` (NotImplemented in C#) is left out until a test needs it. `ResolveFilePath` passes the path through; `ShowFileInFolder` does nothing.
- Presenting goes through a small `ModalPresenter` trait: `AutomationWindow` implements it with a built-in overlay stack, and MatterCAD implements it with its `popup_layer`.
- MatterCAD adds `AutomationFileService: FileService` that opens this dialog. C#'s `CompleteDialog` then ports word for word: wait for focus, `type_text(paths)`, `{Enter}`.

**Live shell**
- **New in `agg-gui-shell`: `ShellSession`.** It does the same window and GPU bring-up as `run`, but returns control to the caller:
  - `pump(timeout)` via winit 0.30's `EventLoopExtPumpEvents::pump_app_events` (Windows, macOS, Linux)
  - `app()` / `app_mut()`, `forwarder()`
  - `request_close()` (runs the host's `on_close_requested`), `force_close()`, `presented_frames()`
  - `set_platform_input_enabled`, `set_platform_deactivation_enabled`.
- `run()` becomes a thin loop over the same internals.
- Live tests are separate `harness = false` binaries (macOS needs the event loop on the main thread). The budget watchdog is a helper thread: it sets `cancel`, and after a grace period it dumps stacks and calls `process::exit`.
- The web shell is out of scope for v1, as it is in C# (wasm can't block). The `UiDriver` trait leaves room for a later async front.

**`StartupFailureLog`:**
- process-wide `Mutex<Option<String>>`
- `record(phase, &dyn Error or panic payload)`, `reset()`, `append_to(msg)`
- an immediate write to the real stderr fd, which libtest's capture does not intercept.

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
| G11 | `Widget::find_named_targets`; ComboBox per-item names and enabled state; menu rows | 24 |
| G12 | Flex reverse directions (RightToLeft, BottomToTop) | 25 |
| G13 | `NumberField` (number-only edit, text-parser hook, `=`-expression flag) | 20 |
| G14 | Mac text-edit key bindings, plus a thread-local `platform` override | 21 |
| G15 | Paint panic containment in `App::paint` (reported through `report_unhandled` with a new `UnhandledOrigin::Paint`), `DrawCtx` state rebalance | 27 |
| G16 | `Widget::scroll_rect_into_view` on ScrollView/TextArea | 17 |
| G17 | `ShellSession` / pump API, input and deactivation gates | 31 |
| G18 | wgpu present-failure frame reset | 33 |

**C# class → Rust file** (`agg-gui-automation/tests/`; tests keep their names in snake_case):

| C# class (count) | Rust file | Notes |
|---|---|---|
| AutomationRunnerTests (11) | `automation_runner_tests.rs` | via `show_window_and_execute_tests`; 3 to go (StaticDelayExpires…, ZeroSecondWaitsReportWhatIsThereNow, WindowLoadTimeIsNotChargedToTheTestBudget, AutomationRunnerTimeoutTest, GetWidgetByNameTestNoRegionSingleWindow, GetWidgetByNameTestRegionSingleWindow, DoubleClickByNameSendsTwoFullClickPairsWithProductionClickCounts and TypeDeliversPunctuationIntoAMultiLineField are ported) |
| AutomationRunnerTests.Winforms (3) | `live/automation_runner_live_tests.rs` | `harness = false`, all desktop OSes |
| FlowLayoutTests (24) | `flow_layout_tests.rs` (+ `flow_layout_anchor_tests.rs` when over 800 lines) | FlowLayoutWidget → FlexColumn/FlexRow; image compares via `image_match` |
| MacTextEditKeyBindingTests (12) | `mac_text_edit_key_binding_tests.rs` | `HeadlessWindow` + `app.on_key_down` |
| MenuTests (1) | `menu_tests.rs` | DropDownList → ComboBox |
| MouseInteractionTests (13) | `mouse_interaction_tests.rs` (+ `mouse_interaction/*.rs`) | all ported |
| PaintExceptionContainmentTests (2) | `paint_exception_containment_tests.rs` | headless; live variant when `live` is on |
| PresentFailureContainmentTests (1) | `live/present_failure_containment_tests.rs` | skipped on a CPU host, as in C# |
| TextEditFocusTests (3) | `text_edit_focus_tests.rs` | all ported |
| TextEditTests (13) | `text_edit_tests.rs` (+ `text_edit/*.rs`) | TextEditWidget → TextField/TextArea; NumEdit → NumberField; 7 ported |
| ThreadStackDumpTests (10) | `thread_stack_dump_tests.rs` | `stack-dump` feature |
| ToolTipTests (7) | `tool_tip_tests.rs` | `Thread.Sleep` → `delay`/virtual clock advance |

## 9. Slice plan (about 25 minutes each, one agent; each ends with `cargo test` green)

| # | Slice | Tests that land |
|---|---|---|
| 17 | TextEdit, part 2 (+ G16, and the image match of slice 23) | MultiLineTests, ScrollingToEndShowsEnd |
| 18 | ToolTips, part 1 | ToolTipInitialOpenTests, ToolTipsShow, ToolTipCloseOnLeave, MoveFromToolTipToToolTip |
| 19 | ToolTips, part 2 | MoveFastFromToolTipToToolTip, MoveFromToolTipToOverlappingWidgetWithNoToolTip, ClearAlsoDropsAToolTipThatIsArmedButNotYetShown |
| 20 | G13 NumberField | NumEditHandlesNonNumberChars, NumEditWithTextParserAcceptsLettersAndCommitsTheParsedValue, NumEditRefusesKeysItCannotRead, NumEditTakesLeadingEqualsOnlyWhenExpressionEntryIsAllowed |
| 21 | G14, part 1 | MacAltArrowsMoveByWord, MacCommandLeftGoesToLineStartNotWordBoundary, MacCommandRightGoesToLineEnd, MacCommandUpAndDownGoToDocumentStartAndEnd, MacAltBackspaceDeletesPreviousWord, MacCommandBackspaceDeletesToLineStart |
| 22 | G14, part 2 | MacShiftComposesWithWordAndLineMotion, MacUnshiftedCommandArrowsCollapseSelection, UseMacKeyBindingsDefaultsToRunningOs, MacHomeAndEndStillGoToLineStartAndEnd, WindowsControlLeftStillJumpsByWord, WindowsControlHomeStillGoesToDocumentStart |
| 23 | Image search, `get_current_screen`, simulated-mouse overlay | ImageWaitsSearchTheGivenRegion |
| 24 | G11 named targets plus ComboBox items | OpenAndCloseMenus |
| 25 | G12, then Flow, part 1 | TopToBottomContainerAppliesExpectedMargin, SpacingClearedAfterLoadPositionsCorrectly, NestedLayoutTopToBottomTests, ChangingChildVisiblityUpdatesFlow, ChangingChildFlowWidgetVisiblityUpdatesParentFlow, NestedLayoutTopToBottomWithResizeTests, LeftToRightTests, RightToLeftTests |
| 26 | Flow, part 2 | NestedMaxFitOrStretchToChildrenParentWidth, NestedMinFitOrStretchToChildrenParentWidth, LeftToRightAnchorLeftBottomTests, AnchorLeftRightTests, NestedFlowWidgetsTopToBottomTests, NestedFlowWidgetsRightToLeftTests, NestedFlowWidgetsLeftToRightTests, FlowWithMaxSizeChildAllocatesToOthers |
| 26b | Flow, part 3 | LeftRightWithAnchorLeftRightChildTests, RightLeftWithAnchorLeftRightChildTests, BottomTopWithAnchorBottomTopChildTests, TopBottomWithAnchorBottomTopChildTests, ChildVisibilityChangeCauseResize, EnsureCorrectSizeOnChildrenVisibleChange, ChildHAnchorPriority, TestVAnchorCenter |
| 27 | G15 paint containment plus UI-panic capture in the runner | AThrowingDrawIsReportedAndTheEventLoopKeepsRunning, ADrawThatThrowsEveryFrameForLongerThanTheMatrixStackIsDeepStillCostsOnlyItsOwnFrames |
| 28 | Close protocol: veto, `close_window` callback, force close, CloseTimeout | VetoedCloseIsForcedAndReportedAsFailure |
| 29 | `StartupFailureLog`, `widget_not_found_message`, `AutomationFileDialog`/`ModalPresenter` | (crate unit tests) |
| 30 | ThreadStackDump, part 1 (retry and dump-writer subprocess) | ACaptureThatLosesTheDumpRaceIsRetakenNotReparsed, ACaptureThatNeverSucceedsGivesUpAndReportsEveryAttempt, WritingADumpCapturesTheDumpWritersLogInsteadOfPrintingIt, ADumpWriterWithMoreOutputThanAPipeHoldsStillFinishes, ADumpWriterThatNeverExitsIsKilledAtTheTimeout |
| 30b | ThreadStackDump, part 2 (minidump capture and walk) | CaptureNamesTheCallingThreadsOwnFrames, ADumpThatCannotWalkItsCapturingThreadIsRejected, RegisteredThreadsAreLabelledInTheDump, ATruncatedThreadCommandIsNotRead, ADumpWithTwoThreadsOnOneStackPointerStillReports |
| 30c | Load and close watchdogs write dumps to `TestResults/` | AnExpectedTimeoutLeavesNoThreadDump |
| 31 | G17 `ShellSession` plus `LiveDriver` | RealMouseInputDoesNotReachAWindowUnderAutomation |
| 32 | Live pump robustness | IdlePumpSurvivesAnotherWindowsTeardown, ShowFromNonPumpThreadReturnsToItsCaller |
| 33 | G18 present-failure reset (agg-gui-wgpu) | APresentThatFailsEveryFrameStillStartsEachNextFrameWhole |

**Tally** (69 still to port): AutomationRunnerTests 11 (8 ported), Winforms 3, Flow 24, Mac 12, Menu 1, Mouse 13 (13 ported), Paint 2, Present 1, TextEditFocus 3 (3 ported), TextEdit 13 (7 ported), ThreadStackDump 10, ToolTip 7, WidgetClick 3 (3 ported): **103**.

**Moving `mattercad-app-test` onto the runner**

| # | Slice | Notes |
|---|---|---|
| M2 | Back `TestHarness` with the runner | `TestHarness` owns a `HeadlessDriver` (settings isolation, sample part, compute workers move to a `MatterCadAppWindow` builder) and exposes `runner()`. `frame`/`settle`/`pump_until_idle` use the driver's frame and `ClockPolicy::Real` when workers are on. Existing helpers delegate. |
| M3 | Delete the name hacks | mattercad-app's View3D and design page implement `find_named_targets` (Object3D controls by C# name, scene children); `CONTROL_KEYS` and `scene_objects`/`controls` string parsing go. `menu_automation` uses menu-row named targets. |
| M4 | `run_test` and `new_part_tab_test` | Equivalents of C# `MatterCADUtilities.RunTest`/`NewPartTabTest` over `show_window_and_execute_tests`, with automatic `mark_test_complete`, C#'s 1280×720 default, and `AutomationFileService` (dialog provider) installed. `complete_dialog`, `add_item_to_bed`, `navigate_to_folder` and friends become extension traits on `AutomationRunner`. |
| M5–M9 | Move the 14 ported test files to the runner API, about 3 files per slice | `click_by_name`/`drag_drop_by_name`/`type_text`/`wait_for_name`/`assert` replace harness calls; `MAX_WAIT_FRAMES` becomes C#'s seconds; teleporting clicks become stepped moves (fix whatever this exposes, test-first). |
| M10 | Remove the old helpers | Delete the deprecated `TestHarness` helpers (`click`, `type_keys`, `drag`, `MAX_WAIT_FRAMES`); update crate docs. That leaves 47 of the 61 C# AutomationTests files, which can then be ported one per slice with the same API. |

## 10. Risks

- **Workers that serve a non-main UI thread** must wake it through that thread's queue (`ui_thread::current_queue()` captured on the UI thread, then `UiQueue::run_on_idle` / `UiQueue::signal_async_state_change`); an unbound worker's `animation::signal_async_state_change` wakes only the main queue's owner. MatterCAD's workers (`running_tasks`, `design_scene`, `library_executor`) call the free function, so a headless test on a non-main thread sees their wakeups only through its own pumping (M2 should capture the queue).
- **Virtual time vs. real worker threads.** Covered by `ClockPolicy::Real` and the `background_busy` hook, so `settle` keeps working.
- **A timed-out body thread keeps running.** It only touches its own thread-locals, and it unwinds at its next runner call.
- **Process-global agg-gui state** (`platform`, input profile, tilt, fullscreen). It needs thread-local overrides (slice 21) or `serial_test` for the few tests that touch it.
- **Headless software paint can be slow on MatterCAD's tree.** Frames paint only when `wants_draw`, so idle `delay`/`wait_for` frames are cheap.
- **The `^` → command-modifier choice** differs from C#'s literal Control on macOS. It is deliberate; `rust_only_caret_is_the_platform_command_modifier` pins it per platform.

### Critical files for implementation
- /Users/larsbrubaker/Development/MatterCAD/Submodules/agg-sharp/GuiAutomation/AutomationRunner.cs
- /Users/larsbrubaker/Development/rust-apps/agg-gui/agg-gui/src/widget/app/pointer.rs (with `app.rs`, `app/keyboard.rs`, `app/path_anchor.rs`)
- /Users/larsbrubaker/Development/rust-apps/agg-gui/agg-gui/src/widget/tree_inspector.rs (`find_widget_by_id`, `find_widget_screen_rect`)
- /Users/larsbrubaker/Development/rust-apps/agg-gui/agg-gui-shell/src/shell_loop.rs (input forwarding, close veto, the pump refactor)
- /Users/larsbrubaker/Development/rust-apps/mattercad-rust/crates/mattercad-app-test/src/lib.rs and src/automation.rs (frame policy and helpers to migrate)
