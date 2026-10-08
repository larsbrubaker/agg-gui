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
- **Order of work:** the remaining slices (7 onward) fill agg-gui's gaps test-first, port the 101 remaining Agg Automation Tests 1:1, then move `mattercad-app-test` onto the runner (section 9).

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
- **Missing pieces:** `is_enabled`, click counts in mouse events (the forwarder counts them; `App` does not take them yet, G9), and paint-panic containment.

**`mattercad-app-test`**
- `TestHarness` builds the real tree headless, sends input straight to `App::on_*`, and keeps its own copy of the shells' frame policy (`input_frame`/`shell_frame`/layout key) instead of `agg_gui::frame_policy` (M2 replaces it).
- Clicks teleport the pointer: there is no stepped move.
- Waits count frames (`MAX_WAIT_FRAMES = 120`) instead of seconds.
- Typing is ad hoc: `type_keys` handles only 4 `{}` tokens and a leading `^`.
- Names of non-widget targets (3D controls, scene objects, menu rows) are found by parsing `properties()` strings.

## 2. Where the code goes

**Crate `agg-gui/agg-gui-automation`** (workspace member; consumers use it as a dev-dependency). It exists with `keys`, `key_mapping`, `typed_key_parser`, `search_region`, `waits` (`static_delay`), `driver` (`HeadlessDriver`, `HeadlessWindow`, `ClockPolicy`, `FrameKind`, `UiDriver`), `probe` (`ProbeWidget`) and `tree_query` (`WidgetHandle`, `find_by_name`, `placement`/`screen_rect`/`clipped_rect`, `actually_visible_on_screen`, `parents`/`children` and their `_of_type` forms); the other modules below are still to come.
- It keeps test-only code (watchdogs, stack dumps, image matching) out of product builds.
- Its optional live mode depends on `agg-gui-shell` (winit and wgpu), which core `agg-gui` must not.
- Features, each added with the slice whose code uses it (none exist yet; headless is the default):
  - `live = ["dep:agg-gui-shell"]` (slice 31)
  - `stack-dump = ["dep:minidump-writer", "dep:minidump-processor", "dep:minidump-unwind"]` (slice 30)

Modules (each under 800 lines, each opening with a purpose comment):

```
src/lib.rs                 crate docs, re-exports
src/runner/mod.rs          AutomationRunner, AutomationConfig, ClickOrigin, ModifierKeys, MarkTestComplete
src/runner/named.rs        Get*/Wait*/NameExists/NamedWidgetExists/ChildExists/GetRegionByName/ScrollIntoView
src/runner/pointer.rs      stepped moves, Click*/RightClick*/DoubleClick*/Drag*/Drop*/MoveToByName/SetMouseCursorPosition
src/runner/keyboard.rs     Type, Press/ReleaseModifierKeys, SelectAll/None
src/runner/waits.rs        Delay, WaitFor, Assert, WaitForPendingUiWork, WaitforDraw (AutomationRunner::static_delay delegates to waits::static_delay)
src/runner/images.rs       ClickImage/DragImage/DropImage/ImageExists/WaitForImage, GetCurrentScreen
src/input.rs               InputMethod trait (IInputMethod) + SimulatedInput (AggInputMethods), MouseConsts→enum
src/pointer_reach.rs       PointerReach
src/tree_query.rs          (exists) WidgetHandle, screen/clip rects, ActuallyVisibleOnScreen, Parents/Children; NamedHit (GetByNameResults) lands with named.rs
src/image_match.rs         FindLeastSquaresMatch over agg_gui::Framebuffer
src/driver/mod.rs          (exists) UiDriver trait, FrameKind, ClockPolicy
src/driver/headless.rs     (exists) HeadlessDriver
src/driver/window.rs       (exists) HeadlessWindow (the "SystemWindow" for tests that use no runner)
src/driver/live.rs         (feature live) LiveDriver over agg_gui_shell::ShellSession
src/execute.rs             show_window_and_execute_tests, RunOptions, AutomationError, watchdogs
src/probe.rs               (exists) ProbeWidget: generic C#-GuiWidget stand-in (name, bounds, colour, event log/callbacks)
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
| `InputType`, `OverrideInputSystem`, static `InputMethod`, `DrawSimulatedMouse` | `RunOptions.input: Box<dyn InputMethod>` (default `SimulatedInput`), `RunOptions.draw_simulated_mouse` |
| `CloseWindowTimeoutSeconds` (15) | `RunOptions.close_window_timeout` |
| `ClickOrigin`, `ModifierKeys`, `InterpolationType` (unused) | `ClickOrigin`, `ModifierKeys` (bitflags). `InterpolationType` is dropped as dead code; noted in the port comment. |
| `CurrentMousePosition()` | `current_mouse_position() -> Point2D` |
| `GetCurrentScreen()` | `get_current_screen() -> Framebuffer`: headless, the last software frame; live, a read-back. |
| `RenderMouse` | `overlay::render_mouse`. The driver draws it after `App::paint`: a circle, green while the left button is down, "S"/"C" for held modifiers, plus the click count. |
| `Dispose`, `KeyDown`/`KeyUp` (throw NotImplemented in C#) | `Drop`. The two key methods are left out. |

**Waits**

| C# | Rust |
|---|---|
| `Delay(s = .2)` | `delay(secs)`: advances driver time in frame-sized steps, one frame each |
| `WaitForPendingUiWork(ms = 250)` | `wait_for_pending_ui_work(max) -> bool` |
| `WaitFor(cond, 5, 10)` | `wait_for(cond, max, interval) -> &mut Self`, plus `wait_until(...) -> bool` |
| `Assert(cond, msg, 5, 10)` | `assert(cond, msg, ...)`: panics with "Require Failed: {msg}" |
| `WaitforDraw(window, 30)` | `wait_for_draw()`: forces a layout+paint frame (headless) or the next presented frame (live) |

**Name lookup**

| C# | Rust |
|---|---|
| `GetWidgetsByName(name, secs, region, onlyVisible)` | `get_widgets_by_name(...) -> Vec<NamedHit>`. `NamedHit { handle: WidgetHandle, offset_hint: Point, target: Option<NamedTarget> }` stands in for `GetByNameResults`. |
| `GetWidgetByName` (3 overloads) | `get_widget_by_name(name, &WaitOpts) -> Option<WidgetHandle>`. It applies PointerReach first, then the largest clipped area; `set_target` sets DebugShowBounds. |
| `GetObjectByName` | `get_object_by_name(...) -> Option<NamedHit>`: the non-widget `NamedTarget` |
| `GetRegionByName` | `get_region_by_name(...) -> Option<SearchRegion>` |
| `NameExists` | `name_exists(name, secs, only_visible)` |
| `NamedWidgetExists(name, region, onlyVisible, predicate)` | `named_widget_exists(...)` |
| `ChildExists<T>` | `child_exists::<T: 'static>(region)`, using `as_any` |
| `WaitForName(name, 2, onlyVisible, predicate)` | `wait_for_name(...) -> bool`, polling once per pumped frame |
| `WaitForWidgetDisappear` | `wait_for_widget_disappear(name, secs) -> bool` |
| `WaitForWidgetEnabled` | `wait_for_widget_enabled(name, secs)`; panics with C#'s message |
| `ScrollIntoView(name, amount)` | `scroll_into_view(name)`: calls `Widget::scroll_rect_into_view` on the nearest scrollable ancestor, falling back to wheel notches |
| `WidgetNotFoundMessage` | `widget_not_found_message(op, name)`; appends `StartupFailureLog` |

**Pointer**

| C# | Rust |
|---|---|
| `ClickByName(name, region, offset, origin, isDoubleClick, secs)` | `click_by_name(name)` plus `click_by_name_with(name, &ClickOpts)`; panics with `widget_not_found_message` |
| `ClickWidget(widget, dbl)` | `click_widget(&WidgetHandle, double)` |
| `RightClickByName` / `RightClickWidget` | `right_click_by_name` / `right_click_widget` |
| `DoubleClickByName` | `double_click_by_name`. Sends down(1), up, down(2) back to back, then the hold and the release, exactly as C# does. |
| `MoveToByName` | `move_to_by_name(...) -> bool` |
| `DragByName` / `DropByName` / `DragDropByName` / `DragWidget(widget, travel)` / `DragToPosition` / `Drop` | Same names in snake_case. `DragWidget`'s `travel` stays Y-down, matching C#'s screen-space add. |
| `SetMouseCursorPosition(window, x, y)` / `(x, y)` | `set_mouse_cursor_position_in_window(x, y)` (Y-up logical) / `set_mouse_cursor_position(x, y)` (Y-down) |
| `ScreenToSystemWindow` / `SystemWindowToScreen` | `window_to_pointer` / `pointer_to_window` (logical Y-up ↔ physical Y-down, using `device_scale`) |

**Keyboard**

| C# | Rust |
|---|---|
| `PressModifierKeys` / `ReleaseModifierKeys` | Same names. Each sends `App::on_modifiers_changed` and the modifier key, then `delay(.2)`. |
| `Type(text)` | `type_text(text)`: strokes from `TypedKeyParser`, then `delay(.2)`. `%{F4}` asks the window to close. |
| `SelectAll` / `SelectNone` | `select_all` (`"^a"`) / `select_none` (`" "`) |

**Images**

| C# | Rust |
|---|---|
| `ClickImage`, `DragImage`, `DropImage`, `DragDropImage`, `ImageExists`, `WaitForImage` (needle as file name or image) | Same names, needle as `ImageNeedle::Path` or `ImageNeedle::Buffer`; least-squares match in `image_match.rs` |
| `DoubleClickImage`, `MoveToImage` (throw NotImplemented in C#) | Left out (no stubs); the port comment names them. |

**Run entry point**

| C# | Rust |
|---|---|
| `ShowWindowAndExecuteTests(window, test, secs = 30, images, closeWindow, timeoutIsTheExpectedOutcome)` | `show_window_and_execute_tests(build, body, RunOptions) -> Result<R, AutomationError>` (section 5) |

**Supporting types**
- `SearchRegion::image(capture)` takes the runner's `get_current_screen` as its capture function.
- **How a stroke is sent** (`TypedKey::agg_key`/`agg_modifiers` give the key and modifiers):
  - `on_key_down(stroke.agg_key(), stroke.agg_modifiers())` then `on_key_up`.
  - agg-gui has no KeyPress. A widget suppressing a KeyPress in C# corresponds to consuming the KeyDown.
- `InputMethod` trait (= `IInputMethod`): `current_mouse_position`, `left_button_down`, `click_count`, `set_cursor_position`, `mouse_event(MouseAction, x, y, clicks)`, `press_modifier_keys`, `release_modifier_keys`, `type_strokes`, `current_screen`. `MouseConsts` becomes `enum MouseAction { LeftDown, LeftUp, RightDown, ... }`.
- `SimulatedInput` keeps C#'s rules:
  - A down reports `clicks` (2 only when stated). Every up reports 1.
  - Moves go to the window. A down or up goes to the window only if the pointer is inside it.

## 4. How simulated input gets in

**Rule:** the runner never calls widget `on_event` directly. Everything goes through the code shells use, so nothing that hover, capture, focus, modals, tooltips or the on-screen keyboard do can be bypassed.

1. **`agg_gui::shell_input::InputForwarder`** owns the shell-neutral bookkeeping (cursor, held buttons and modifiers, click counting by `ClickPolicy` or `ClickCount::Explicit`, the real-input gate via `InputSource::Platform`/`Simulated`) and calls the `App::on_*` entry points; agg-gui-shell and the web shell feed it `ForwarderEvent`s. `SimulatedInput` produces the same `ForwarderEvent`s (`forwarder.simulated(...)`), so headless and live clicks run identical code. Once G9 lands, the forwarder passes its click count to `App::on_mouse_down_clicks`.
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
  1. `build()` creates the `AutomationWindow`: root widget, logical size, `on_load`, `on_close_requested` (the veto), `on_closed`.
  2. The driver comes up and the first paint fires `on_load` (= Load).
  3. A Loaded message is sent and `body` runs inside `catch_unwind`.
  4. If `require_test_completion` is set and `mark_test_complete()` was not called, the run fails with C#'s message.
  5. The close phase runs (below), then the result is sent.
- **The calling thread is the watchdog:**
  - `recv_timeout(max(budget, 30 s))` waits for Loaded. This is the bring-up budget. Its load watchdog dumps stacks 2 s before the end unless `timeout_is_the_expected_outcome`.
  - It then waits `secs_to_test_failure` (wall clock, starting at Loaded) for Done.
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

| C# | Headless |
|---|---|
| `WaitForPendingUiWork(max)` | One frame. The sentinel is the drain, so it returns true. `max <= 0` returns false, as in C#. |
| `PaceMouseMove` | Per step, `WaitForPendingUiWork(remaining of TimeToMoveMouse)`, so 5 frames = 50 virtual ms per move |
| `HoldButton` | `WaitForPendingUiWork(UpDelay)`: one frame |
| `WaitforDraw` | A forced layout+paint frame |
| `Delay(s)` | `ceil(s / frame_interval)` frames |
| `WaitFor` / `Assert(cond, max, interval)` | Look; while virtual time is under `max`, pump frames worth `interval` and look again; the answer is the final look. A 5 s wait that never comes true costs 500 cheap idle frames. |
| `WaitForName` / `Disappear` / `Enabled` | Poll with `WaitForPendingUiWork(50 ms)`, as C# does |
| ClickWidget tail | Draw, draw, `Delay(.2)`, exactly as C# |
| `Type` | Strokes delivered, one frame, `Delay(.2)` |

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

**The tree walk** (`tree_query::collect_named`) walks `root` with the same transform chain `find_widget_screen_rect` uses: bounds origin plus `inspector_child_transform`. For each match it records:
- `WidgetHandle`: the path plus a per-element identity anchor. This makes the agg-gui `path_anchor` resolution public as `agg_gui::widget::WidgetAnchor`, so a handle follows reorders between pumps.
- `screen_rect` and `clipped_rect`: intersection with every ancestor's `clip_children_rect` and the viewport.
- `offset_hint`: the local center, or the target's rect center.

**`ActuallyVisibleOnScreen`:** every ancestor `is_visible()`, a non-empty clipped rect, and still attached (the handle resolves). `onlyVisible` filters on it.

**Choosing among same-named widgets** (`GetWidgetByName`): `PointerReach::prefer_reachable` keeps only widgets where every ancestor's `hit_test(center mapped into ancestor)` is true (the agg-gui counterpart of `PositionWithinLocalBounds`), else keeps all of them. Then the largest clipped area wins.

**Search regions:**
- A `SearchRegion.screen_rect` is Y-down, in window pixels.
- A widget counts as in the region when `ScreenRectangle::intersection` of the region and its screen rect is non-empty.
- `get_region_by_name` turns the named widget's rect into a region.
- Image searches use `region.image`, or a fresh whole-window capture.

**Reading widget state from tests** (C# reads `field.Text`, `IsOpen`, `ContainsFocus` straight off objects):
- Test helpers (over `Widget::as_any`/`as_any_mut`):
  - `runner.with_widget::<TextField, _>("field", |f| f.text())`
  - `runner.with_handle_mut(...)`
  - `runner.contains_focus(&handle)`, which needs a new public `App::focused_path()`
  - `runner.under_mouse_state(&handle)`
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
| G8 | Hovered chain query, `Widget::is_enabled` | 8 |
| G9 | Explicit click counts: `App::on_mouse_down_clicks`, `event::current_click_count()`, `is_double_click` that remembers the down, `MultiClickTracker` honouring the given count | 11 |
| G10 | "First under mouse" events (`MouseOver`/`MouseOut`) plus an `UnderMouseState` query, next to the existing bounds Enter/Leave | 13 |
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
| AutomationRunnerTests (11) | `automation_runner_tests.rs` | via `show_window_and_execute_tests`; 10 to go (StaticDelayExpires… is ported) |
| AutomationRunnerTests.Winforms (3) | `live/automation_runner_live_tests.rs` | `harness = false`, all desktop OSes |
| FlowLayoutTests (24) | `flow_layout_tests.rs` (+ `flow_layout_anchor_tests.rs` when over 800 lines) | FlowLayoutWidget → FlexColumn/FlexRow; image compares via `image_match` |
| MacTextEditKeyBindingTests (12) | `mac_text_edit_key_binding_tests.rs` | `HeadlessWindow` + `app.on_key_down` |
| MenuTests (1) | `menu_tests.rs` | DropDownList → ComboBox |
| MouseInteractionTests (13) | `mouse_interaction_tests.rs` | `ProbeWidget` for GuiWidget; RadioButton → RadioGroup buttons; 12 to go (ExtensionMethodsTests is ported) |
| PaintExceptionContainmentTests (2) | `paint_exception_containment_tests.rs` | headless; live variant when `live` is on |
| PresentFailureContainmentTests (1) | `live/present_failure_containment_tests.rs` | skipped on a CPU host, as in C# |
| TextEditFocusTests (3) | `text_edit_focus_tests.rs` | |
| TextEditTests (13) | `text_edit_tests.rs` | TextEditWidget → TextField/TextArea; NumEdit → NumberField |
| ThreadStackDumpTests (10) | `thread_stack_dump_tests.rs` | `stack-dump` feature |
| ToolTipTests (7) | `tool_tip_tests.rs` | `Thread.Sleep` → `delay`/virtual clock advance |
| WidgetClickTests (3) | `widget_click_tests.rs` | |

## 9. Slice plan (about 25 minutes each, one agent; each ends with `cargo test` green)

| # | Slice | Tests that land |
|---|---|---|
| 7 | `execute.rs`: thread per run, Loaded/bring-up, wall budget plus `cancel`, `catch_unwind`, MarkTestComplete, `on_load` | AutomationRunnerTimeoutTest, WindowLoadTimeIsNotChargedToTheTestBudget |
| 8 | Name lookup and waits (`named.rs`, `waits.rs`, PointerReach), G8 `is_enabled` | ZeroSecondWaitsReportWhatIsThereNow |
| 9 | `SimulatedInput` plus stepped moves and pacing, Click*/RightClick*/MoveToByName/SetMouseCursorPosition | GetWidgetByNameTestNoRegionSingleWindow, GetWidgetByNameTestRegionSingleWindow |
| 10 | Click semantics on probe widgets and Button | WidgetClick: ClickFiresOnCorrectWidgets, ClickSuppressedOnExternalMouseUp, ClickSuppressedOnMouseUpWithinChild2 |
| 11 | G9 click counts plus `double_click_by_name` | DoubleClickByNameSendsTwoFullClickPairsWithProductionClickCounts |
| 12 | Drag*/Drop* | Mouse: DoClickButtonInWindow, RadioButtonSiblingsAreChildren, ValidateSimpleLeftClick, ValidateOnlyTopWidgetGetsLeftClick |
| 13 | G10 under-mouse state | Mouse: ValidateSimpleMouseUpDown, ValidateOnlyTopWidgetGetsMouseUp, ValidateEnterAndLeaveEvents, ValidateEnterAndLeaveEventsWhenNested |
| 14 | Capture and overlap behaviour | Mouse: ValidateEnterAndLeaveEventsWhenCoverd, ValidateEnterAndLeaveInOverlapArea, MouseCapturedSpressesLeaveEvents, MouseCapturedSpressesLeaveEventsInButtonsSameAsRectangles |
| 15 | Keyboard (`type_text`, modifiers, `select_all`/`none`) | TypeDeliversPunctuationIntoAMultiLineField; TextEditFocus: VerifyFocusMakesTextWidgetEditable, VerifyFocusProperty, SelectAllOnFocusCanStillClickAfterSelection |
| 16 | TextEdit, part 1 | CorectLineCounts, TextEditTextSelectionTests, TextSelectionWithShiftClick, TextChangedEventsTests, TextEditGetsFocusTests, AddThenDeleteCausesNoVisualChange |
| 17 | TextEdit, part 2 (+ G16) | MultiLineTests, TextEditingSpecialKeysWork, ScrollingToEndShowsEnd |
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

**Tally** (101 still to port): AutomationRunnerTests 11 (1 ported), Winforms 3, Flow 24, Mac 12, Menu 1, Mouse 13 (1 ported), Paint 2, Present 1, TextEditFocus 3, TextEdit 13, ThreadStackDump 10, ToolTip 7, WidgetClick 3: **103**.

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
