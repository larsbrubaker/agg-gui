# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [0.5.0] - Unreleased

### Added

- Idle sleep: the frame loop stops requesting animation frames while nothing
  needs one (no repaint, draw or layout request, animation, held pointer,
  connected gamepad, pending tilt or fullscreen request, or host poll), and
  arms one `setTimeout` for the earliest scheduled draw or queued idle work
  (`ui_thread::next_due`, asked directly so work posted mid-tick is never
  lost to the paint's flag clear) instead of ticking until it. Input, agg-gui requests made outside a tick (through the new
  `agg_gui::frame_demand` hook), a canvas `ResizeObserver`, DPR and
  fullscreen changes, `gamepadconnected`, visibility and device loss wake it.
  `request_tick()` asks for one tick from an export; a host that polls in
  `on_tick` returns `true` from the new `WebShellHost::wants_tick`, which
  `demo-wgpu`'s deprecated `web_shell::start` wrapper does for its per-frame
  closure.
- The canvas's render context records its device's generation
  (`agg_gui_wgpu::next_device_generation`, taken anew for every device-loss
  rebuild), so custom render passes see
  `WgpuCustomRenderCtx::device_generation`. Frame completion times
  (`WgpuCustomRenderCtx::last_frame_completion`) come from the render context
  as on native.
- `WebShellConfig::with_adapter_check(|adapter| -> Result<(), String>)` /
  `adapter_check`: the app checks the adapter the browser handed out (downlevel
  flags, texture format features) before the device is requested, and on every
  device-loss rebuild. An `Err` stops start-up with the app's own message as the
  new `WebShellError::AdapterRejected`, through the fatal path (console,
  `on_fatal`, the panel). `GpuInfo::adapter` hands the adapter to the app
  builder and `on_gpu_rebuilt`, and the render context records it, so custom
  render passes see its downlevel capabilities.

- `WebShellConfig::with_claimed_shortcut(key)` / `claimed_shortcuts`: Ctrl/Cmd shortcuts the app
  handles itself and takes from the browser (`'s'` for Save, so "Save Page As" does not open),
  applied at start through agg-gui's `claim_web_shortcut`. Empty by default.

- `WebShellConfig::with_keyboard_needs_focus`: for a canvas embedded in a
  larger page, keys and the clipboard reach the app only while the canvas (or
  the shell's IME input) has focus, and a press on the canvas focuses it
  without scrolling the page (a canvas with no `tabindex` gets
  `tabindex="0"` so it can take focus). With focus elsewhere the page keeps its own
  keyboard scrolling, find and copy. Off by default (a full-window app owns
  every key). Built on agg-gui's new
  `web_adapter::install_keyboard_listeners_while`.

- The canvas surface format follows agg-gui-wgpu's `choose_surface_format`:
  Bgra8Unorm whichever of the two 8-bit orders the browser lists first, so a
  browser capture is the same pixels as a native window and a headless golden.

- Window activation: the page's `blur` / `focus` on `window` (a tab or app
  switch) reach the app as `App::on_window_deactivated` /
  `App::on_window_activated`; a drag in progress on blur ends with
  `Event::MouseCaptureLost`.

- File drag-and-drop on the canvas: `dragenter` / `dragover` of a drag that
  carries files are accepted (so the browser delivers the drop instead of
  opening the file) and sent as `App::on_file_drag_hover` with no paths;
  `dragleave` sends `App::on_file_drag_leave`; `drop` ends the hover at once,
  reads every file's bytes, and sends them together through
  `App::on_file_data_dropped` at the drop position.

- `Frame::needs_layout` honours `agg_gui::animation::request_layout()`: a
  pending request lays out the frame even when the layout key is unchanged.

- First release: the browser counterpart of `agg-gui-shell`, extracted from
  `demo-wgpu`'s `web_shell` and merged with AtomArtist's hand-rolled web shell.
- `start(WebShellConfig, builder)` — canvas lookup, client-platform detection
  (`?agg_input=mobile|desktop` override), async GPU bring-up, then the app
  builder (after the GPU exists, like the native shell), and the
  `requestAnimationFrame` loop.
- `Backend::{WebGpu, WebGl2, PreferWebGpu}` — the consumer picks the browser
  graphics API; `PreferWebGpu` probes WebGPU before the canvas is bound so the
  WebGL2 fallback is clean. WebGPU is behind the default `webgpu` feature.
- `WebShellHost` mirroring `agg_gui_shell::ShellHost` (`on_frame`, `paint`,
  `after_paint`, `on_geometry_changed`, `on_idle`, `on_gpu_rebuilt`) plus
  web-only `on_tick` (every rAF) and `on_page_hide` (visibilitychange/pagehide
  flush).
- `RedrawPolicy::{Reactive, Continuous}` with scheduled-deadline wake-ups, and
  `FirstPaintGate` — the first frame always paints, fixing the
  blank-page-until-resize bug AtomArtist hit.
- DOM pointer (mouse / pen / multi-touch), wheel, pointer-leave and
  context-menu forwarding; keyboard + clipboard via `agg_gui::web_adapter`;
  window-level pointer-release resync behind `WebShellControl::pointer_idle`.
- DPR tracking and canvas backing-store sizing every tick, clamped to the
  device texture limit; surface-acquire recovery; device-loss rebuild.
- `WebShellConfig::offscreen_scene` — render through a copyable scene texture
  so `WgpuGfxCtx::capture_screenshot` works on the web.
- `LocalStorageSettings` / `MemorySettings` / `SettingsAutoSave` — optional
  diff-guarded, pointer-idle-gated settings persistence.
- `agg_gui::fullscreen` (with mobile orientation lock), `agg_gui::tilt` and
  `agg_gui::gamepad` plumbing, and a fatal-error panel in place of the canvas.
- Fatal-error path shared by every failure (sync start errors included):
  `console.error`, `WebShellConfig::with_on_fatal` hook, and the panel, with
  `with_app_name` / `with_fatal_message` wording and a `prefers-color-scheme`
  dark style (or `with_fatal_panel_class` for page-owned CSS).
- `WebShellConfig::with_required_features` (`WebShellError::MissingFeatures`
  when the adapter lacks one).
- `WebShellHost::after_present` hook and wasm `has_presented()`.
- `WebShellError::AlreadyStarted` (a second `start` on the page) and
  `WebShellError::DeviceLost`.
- Re-exports of `agg_gui`, `agg_gui_wgpu` and (wasm) `web_sys`.

### Changed (relative to the first draft of this release)

- Internals are no longer public: `dom_math`, `FirstPaintGate`,
  `wants_paint`, `layout_key` / `LayoutKey`, `device_limits`,
  `pick_surface_format`. `Backend` and `RedrawPolicy` are `#[non_exhaustive]`.
- Mouse/pen moves no longer force a repaint; they paint through
  `App::wants_draw()` like the pre-extraction `demo-wgpu` shell.
- **Breaking for consumers that set `default-features = false`:** WebGL2 is
  the new `webgl` feature, on by default next to `webgpu` (forwarding to
  agg-gui-wgpu's `webgl`). A WebGL2-only app that turned default features off
  must now ask for it: `default-features = false, features = ["webgl"]`;
  without it `Backend::WebGl2` fails with `BackendNotCompiled`. A WebGPU-only
  app builds with
  `default-features = false, features = ["webgpu"]` and leaves wgpu's GLES
  backend and GLSL shader writer (about 2.4 MB) out of its wasm.
  `Backend::wgpu_backends` returns only what is compiled in, so asking for
  WebGL2 without the feature is `WebShellError::BackendNotCompiled`.

### Fixed

- A slow GPU no longer builds a backlog of frames: the frame loop counts each
  presented frame until its `onSubmittedWorkDone()` resolves and keeps at
  most two on the GPU, one while the device's recent frames take longer than
  50 ms. While the GPU is full it skips painting and keeps the redraw
  pending, so the first tick after the GPU catches up paints the latest
  state. Ticks, `on_tick` and input run at the display rate throughout. On
  SwiftShader at device scale 3, dragging the camera of a 3D view had the
  browser's animation frames slow to about 2 a second without a limit and
  under 5 with two in flight; with one they stay at 60 (309 mouse moves
  handled in 8 s against 16). A real GPU (Metal) paints 57-60 frames a second
  with any limit. A frame whose completion is overdue (four times the
  slowest recent frame, between 1 and 10 s) stops being waited for, with one
  console warning per stall, and teaches the wait, so a GPU slower than a
  second a frame is treated as slow rather than stalled from its second
  frame on. Each frame's time is its own time on the GPU (from when the frame
  ahead of it finished), and a slow device goes back to two in flight only
  below 35 ms, so the limit does not switch every frame. The count and the
  learned frame times start again whenever a device is installed, so a lost
  or wedged GPU cannot stop painting for good.
- Firefox paints every frame of an animation again instead of about ten a
  second. Firefox's WebGPU reports a finished frame when it next presents one
  or at its own 100 ms device poll, so a lone frame (the last before a pause)
  was reported up to 100 ms late, timed as a slow GPU, and the gate dropped
  to one frame in flight; with nothing presented behind it, every frame then
  waited for the poll. A lone frame slower than 50 ms now holds painting and
  asks the browser an empty `onSubmittedWorkDone` (no submit); two answered
  late in a row show the delay is the browser's, and from then on lone frames
  are not timed and two frames stay in flight. A prompt answer means the
  frame really was slow, and it is learned as before. Measured on an M4 Max
  with MatterCAD's editor sliding in: Firefox 157 painted 6 frames in the
  half-second slide before and 24 after, the same as Chrome. Only the
  browser's WebGPU is asked; wgpu's WebGL2 backend is unchanged.
- Every frame starts from the theme's background: an area no widget paints
  shows the background, never the canvas's initial transparent black or an
  earlier frame's drawing.
- Canvas resized every frame when `client × DPR` exceeded the GPU's
  `max_texture_dimension_2d`: the backing store is now fitted once (scale
  reduced uniformly) and pointer mapping uses the fitted scale.
- A failing device-loss rebuild retried every frame; it now backs off
  exponentially and shows the fatal panel after 5 attempts. A lost WebGL2
  context is reported instead of leaving a dead canvas.
- `WebShellControl::pointer_idle` was true during touch drags; touch contacts
  are now counted.
- `on_geometry_changed` never delivered the boot geometry; it now fires once
  right after the app is built.
- A global `set_redraw_policy` called inside `on_idle` was overwritten.
- `start` errors detected synchronously (canvas not found) were silent unless
  the caller logged them; they now take the fatal path too.
- Frames are presented through `WgpuGfxCtx::present`, which releases the
  context's stashed back-buffer handle before presenting (the DX12 resize
  crash fixed in agg-gui-wgpu 0.5.3, the minimum version this crate requires).
