# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [0.5.0] - Unreleased

### Added

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

### Fixed

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
